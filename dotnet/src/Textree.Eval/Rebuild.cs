using System.Text.Json.Nodes;
using Microsoft.Extensions.AI;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// A tree built again by ingest alone: the documents of some folders of an organized tree are
/// ingested one by one into an empty folder, each plan applied, in several orders. The tree on disk
/// after the last one is compared with the original: how documents are grouped (BCubed F1 over the
/// folder that holds most of each document's paragraphs at the end) and how the trees differ (NTED —
/// <see cref="Nted"/> — over folders with the documents as leaves). Where the first piece of each
/// document landed is reported beside it. No threshold: this is the baseline the reorganization
/// stage is measured from. The per-step gates are judged without untouched paths (an empty folder
/// has none), so G5 is not applicable there.
/// </summary>
public static class Rebuild
{
    public static async Task<JsonObject> RunAsync(string root, IReadOnlyList<string> folders, IChatClient chat, int seed, CancellationToken ct = default)
    {
        var fingerprint = Workspace.Fingerprint(root);
        var source = Hierarchy.Read(new Folder(root));
        var docs = source.Documents
            .Where(d => folders.Any(f => Gates.Under(d.Path, f)))
            .Where(d => source.FolderDocument(d.Folder)?.Path != d.Path)
            .OrderBy(d => d.Path, StringComparer.Ordinal)
            .ToList();
        var random = new Random(seed);
        var orders = new Dictionary<string, List<Doc>>
        {
            ["path_order"] = docs,
            ["reverse"] = Enumerable.Reverse(docs).ToList(),
            ["shuffled"] = docs.OrderBy(_ => random.Next()).ToList(),
        };
        var results = new JsonObject();
        var finals = new List<Dictionary<string, string>>();
        var gateFailures = new SortedDictionary<string, int>(StringComparer.Ordinal);
        foreach (var (name, order) in orders)
        {
            var empty = Path.Combine(Path.GetTempPath(), "textree-rebuild-" + Guid.NewGuid().ToString("N")[..8]);
            Directory.CreateDirectory(empty);
            using var ws = new Workspace(empty);
            Directory.Delete(empty);
            var folder = new Folder(ws.Root);
            var landing = new Dictionary<string, string>(StringComparer.Ordinal);
            var outcomes = new SortedDictionary<string, int>(StringComparer.Ordinal);
            var calls = 0;
            // A document that could not be placed, with why — an outcome count alone does not say.
            var failures = new JsonArray();
            foreach (var d in order)
            {
                var before = Workspace.Files(ws.Root);
                var r = await new Ingester(folder, chat).IngestAsync(new IngestRequest(d.Text), null, ct);
                outcomes[r.Outcome] = outcomes.GetValueOrDefault(r.Outcome) + 1;
                if (r.Outcome == "failed")
                {
                    failures.Add(new JsonObject
                    {
                        ["doc"] = d.Path,
                        ["violations"] = new JsonArray((r.Violations ?? []).Select(v => (JsonNode)v).ToArray()),
                    });
                }
                calls += r.Trace.ModelCalls;
                var applied = r.Plan is not null && folder.Apply(r.Plan).Ok;
                var gates = Gates.Judge(before, Workspace.Files(ws.Root), d.Text, r, [], applied, null);
                foreach (var (g, v) in gates.Where(kv => kv.Value.StartsWith("fail", StringComparison.Ordinal)))
                {
                    gateFailures[g] = gateFailures.GetValueOrDefault(g) + 1;
                }
                if (r.Placement.Count > 0)
                {
                    landing[d.Path] = JsonLinesSettlements.FolderOf(r.Placement.OrderBy(p => p.Piece).First());
                }
            }
            var truth = docs.ToDictionary(d => d.Path, d => d.Folder, StringComparer.Ordinal);
            var built = Hierarchy.Read(folder);
            var final = FinalFolders(docs, built);
            finals.Add(final);
            results[name] = new JsonObject
            {
                ["outcomes"] = Json.Object(outcomes),
                ["failures"] = failures,
                ["bcubed"] = BCubed(truth, final),
                ["nted"] = Math.Round(Nted.Normalized(Nted.TreeOf(truth), Nted.TreeOf(final)), 4),
                ["documents_not_found"] = docs.Count - final.Count,
                ["first_piece_landing"] = new JsonObject
                {
                    ["bcubed"] = BCubed(truth, landing),
                    ["nted"] = Math.Round(Nted.Normalized(Nted.TreeOf(truth), Nted.TreeOf(landing)), 4),
                },
                ["folders_built"] = built.Documents.Select(x => x.Folder).Distinct().Count(),
                ["documents_built"] = built.Documents.Count,
                ["model_calls"] = calls,
            };
        }
        // How far the orders disagree with each other.
        var spread = new JsonArray();
        var spreadNted = new JsonArray();
        for (var i = 0; i < finals.Count; i++)
        {
            for (var j = i + 1; j < finals.Count; j++)
            {
                spread.Add(BCubed(finals[i], finals[j])["f1"]!.DeepClone());
                spreadNted.Add(Math.Round(Nted.Normalized(Nted.TreeOf(finals[i]), Nted.TreeOf(finals[j])), 4));
            }
        }
        return new JsonObject
        {
            ["kind"] = "rebuild",
            ["folders"] = new JsonArray(folders.Select(f => (JsonNode)f).ToArray()),
            ["documents"] = docs.Count,
            ["original_folders"] = docs.Select(d => d.Folder).Distinct().Count(),
            ["orders"] = results,
            ["order_agreement_bcubed_f1"] = spread,
            ["order_distance_nted"] = spreadNted,
            ["measured_on"] = "the tree on disk after the last document (first_piece_landing: where each document's first piece was placed)",
            ["nted_kind"] = "constrained unordered edit distance (Zhang 1996) over folders with the documents as leaves, divided by the size of both trees; never below the unconstrained distance",
            ["gate_failures"] = Json.Object(gateFailures),
            ["corpus_unchanged"] = Workspace.Fingerprint(root) == fingerprint,
        };
    }

    /// <summary>
    /// For each source document, the folder of the built tree that holds most of its body paragraphs
    /// (ties: the first in ordinal order); a document none of whose paragraphs is found is left out.
    /// </summary>
    public static Dictionary<string, string> FinalFolders(IReadOnlyList<Doc> sources, Hierarchy built)
    {
        var where = new Dictionary<string, List<string>>(StringComparer.Ordinal);
        foreach (var d in built.Documents)
        {
            foreach (var key in Paragraphs.Split(d.Text).Where(p => Paragraphs.HeadingLevel(p) == 0).Select(Paragraphs.Key).Distinct())
            {
                (where.TryGetValue(key, out var l) ? l : where[key] = []).Add(d.Folder);
            }
        }
        var result = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var s in sources)
        {
            var votes = Paragraphs.Split(s.Text).Where(p => Paragraphs.HeadingLevel(p) == 0).Select(Paragraphs.Key).Distinct()
                .SelectMany(k => where.TryGetValue(k, out var l) ? l.Distinct() : [])
                .GroupBy(f => f, StringComparer.Ordinal)
                .OrderByDescending(g => g.Count()).ThenBy(g => g.Key, StringComparer.Ordinal)
                .FirstOrDefault();
            if (votes is not null)
            {
                result[s.Path] = votes.Key;
            }
        }
        return result;
    }

    /// <summary>BCubed precision, recall and F1 of a clustering against a reference (items missing from <paramref name="found"/> are singletons).</summary>
    public static JsonObject BCubed(IReadOnlyDictionary<string, string> truth, IReadOnlyDictionary<string, string> found)
    {
        var items = truth.Keys.ToList();
        string F(string i) => found.TryGetValue(i, out var f) ? f : "\u0000" + i;
        double p = 0, r = 0;
        foreach (var i in items)
        {
            var sameFound = items.Where(j => F(j) == F(i)).ToList();
            var sameTruth = items.Where(j => truth[j] == truth[i]).ToList();
            p += (double)sameFound.Count(j => truth[j] == truth[i]) / sameFound.Count;
            r += (double)sameTruth.Count(j => F(j) == F(i)) / sameTruth.Count;
        }
        p /= items.Count;
        r /= items.Count;
        var f1 = p + r == 0 ? 0 : 2 * p * r / (p + r);
        return new JsonObject { ["precision"] = Math.Round(p, 4), ["recall"] = Math.Round(r, 4), ["f1"] = Math.Round(f1, 4) };
    }
}
