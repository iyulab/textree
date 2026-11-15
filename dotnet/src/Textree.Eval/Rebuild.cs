using System.Text.Json.Nodes;
using Microsoft.Extensions.AI;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// A tree built again by ingest alone: the documents of some folders of an organized tree are
/// ingested one by one into an empty folder, each plan applied, in several orders. The tree that
/// results is compared with the original: how documents are grouped (BCubed F1 over the folder each
/// document's text ends up in) and how the trees differ (NTED — <see cref="Nted"/> — over folders
/// with the documents as leaves). No threshold: this is the baseline the reorganization stage is
/// measured from.
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
            foreach (var d in order)
            {
                var before = Workspace.Files(ws.Root);
                var r = await new Ingester(folder, chat).IngestAsync(new IngestRequest(d.Text), null, ct);
                outcomes[r.Outcome] = outcomes.GetValueOrDefault(r.Outcome) + 1;
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
            finals.Add(landing);
            var built = Hierarchy.Read(folder);
            results[name] = new JsonObject
            {
                ["outcomes"] = Json.Object(outcomes),
                ["bcubed"] = BCubed(truth, landing),
                ["nted"] = Math.Round(Nted.Normalized(Nted.TreeOf(truth), Nted.TreeOf(landing)), 4),
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
            ["nted_kind"] = "constrained unordered edit distance (Zhang 1996) over folders with the documents as leaves, divided by the size of both trees; never below the unconstrained distance",
            ["gate_failures"] = Json.Object(gateFailures),
            ["corpus_unchanged"] = Workspace.Fingerprint(root) == fingerprint,
        };
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
