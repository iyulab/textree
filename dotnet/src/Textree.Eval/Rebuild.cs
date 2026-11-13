using System.Text;
using System.Text.Json.Nodes;
using Microsoft.Extensions.AI;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// A tree built again by ingest alone: the documents of some folders of an organized tree are
/// ingested one by one into an empty folder, each plan applied, in several orders. The tree that
/// results is compared with the original: how documents are grouped (BCubed F1 over the folder each
/// document's text ends up in) and how the folder trees differ (the share of folder paths that are
/// in one tree but not the other — the tree edit distance when nodes are only added or removed,
/// normalized by the size of both trees). No threshold: this is the baseline the reorganization
/// stage is measured from.
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
                ["folder_path_distance"] = PathDistance(FoldersOf(source, docs), built.Documents.SelectMany(x => Ancestors(x.Folder)).ToHashSet(StringComparer.Ordinal)),
                ["folders_built"] = built.Documents.Select(x => x.Folder).Distinct().Count(),
                ["documents_built"] = built.Documents.Count,
                ["model_calls"] = calls,
            };
        }
        // How far the orders disagree with each other.
        var spread = new JsonArray();
        for (var i = 0; i < finals.Count; i++)
        {
            for (var j = i + 1; j < finals.Count; j++)
            {
                spread.Add(BCubed(finals[i], finals[j])["f1"]!.DeepClone());
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

    private static HashSet<string> FoldersOf(Hierarchy h, List<Doc> docs) =>
        docs.SelectMany(d => Ancestors(d.Folder)).ToHashSet(StringComparer.Ordinal);

    private static IEnumerable<string> Ancestors(string folder)
    {
        yield return "";
        for (var f = folder; f.Length > 0; f = Hierarchy.DirOf(f))
        {
            yield return f;
        }
    }

    /// <summary>Folder paths in one tree but not the other, over the folders of both (0 = the same folder tree).</summary>
    private static double PathDistance(HashSet<string> a, HashSet<string> b)
    {
        var na = a.Select(x => x.Normalize(NormalizationForm.FormC)).ToHashSet();
        var nb = b.Select(x => x.Normalize(NormalizationForm.FormC)).ToHashSet();
        var diff = na.Except(nb).Count() + nb.Except(na).Count();
        return Math.Round((double)diff / (na.Count + nb.Count), 4);
    }
}
