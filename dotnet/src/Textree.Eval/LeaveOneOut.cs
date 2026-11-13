using System.Diagnostics;
using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Gil.Review;
using Microsoft.Extensions.AI;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// Placement measured on a tree people organized: take one document out, hide what would tell
/// where it was, ingest its text, and see where it lands. The original place is the answer.
/// Every arm (the harness and the baselines) sees the same masked tree.
/// </summary>
public static partial class LeaveOneOut
{
    public sealed record Settings(int PerStratum, ulong Seed, int Runs, int MinChars, bool Unmasked, IReadOnlySet<string> Arms, string? Only = null);

    /// <summary>One document taken out: where it was, its text with location hints removed, and the masked tree.</summary>
    private sealed record Case(string Path, string Folder, string Stratum, string Text);

    public static async Task<JsonObject> RunAsync(string root, IChatClient chat, Settings settings, CancellationToken ct = default)
    {
        var fingerprint = Workspace.Fingerprint(root);
        using var baseline = new Workspace(root);
        var hierarchy = Hierarchy.Read(new Folder(baseline.Root));

        // The population: documents with enough text that are not a folder's own document.
        var population = hierarchy.Documents
            .Where(d => hierarchy.FolderDocument(d.Folder)?.Path != d.Path)
            .Where(d => Body(d.Text).Length >= settings.MinChars)
            .ToList();
        var strata = population.GroupBy(d => StratumOf(d.Path))
            .OrderBy(g => g.Key, StringComparer.Ordinal)
            .Select(g => new PopulationStratum(g.Key, g.Select(d => d.Path).Order(StringComparer.Ordinal).ToList()))
            .ToList();
        var sample = ReviewSampler.DrawPilot(strata, settings.PerStratum, settings.Seed);
        var picked = sample.Strata.SelectMany(s => s.Selected.Select(id => (Stratum: s.Name, Path: id)))
            .Where(p => settings.Only is null || p.Path == settings.Only).ToList();

        var arms = new Dictionary<string, List<JsonObject>>(StringComparer.Ordinal);
        void Add(string arm, JsonObject row)
        {
            if (!arms.TryGetValue(arm, out var list))
            {
                arms[arm] = list = [];
            }
            list.Add(row);
        }
        var gateFailures = new SortedDictionary<string, int>(StringComparer.Ordinal);

        foreach (var (stratum, path) in picked)
        {
            var doc = hierarchy.Document(path)!;
            foreach (var masked in settings.Unmasked ? new[] { true, false } : [true])
            {
                using var ws = new Workspace(baseline.Root);
                var text = TakeOut(ws.Root, doc, masked);
                var c = new Case(path, doc.Folder, stratum, text);
                var remaining = Hierarchy.Read(new Folder(ws.Root));
                var suffix = masked ? "" : ".unmasked";

                if (masked && settings.Arms.Contains("b0"))
                {
                    Add("b0", Row(c, "", null));
                }
                if (masked && settings.Arms.Contains("b1"))
                {
                    Add("b1", Row(c, Baselines.Nearest(remaining, text), null));
                }
                if (masked && settings.Arms.Contains("gil"))
                {
                    Add("gil", Baselines.Gil(remaining, c.Path, text));
                }
                for (var run = 1; run <= settings.Runs; run++)
                {
                    if (masked && settings.Arms.Contains("b2"))
                    {
                        var (landing, calls, tokens) = await Baselines.SingleShotAsync(remaining, text, chat, ct);
                        var row = Row(c, landing, null);
                        row["run"] = run;
                        row["model_calls"] = calls;
                        row["tokens"] = tokens;
                        Add("b2", row);
                    }
                    if (settings.Arms.Contains("harness"))
                    {
                        var row = await HarnessAsync(ws.Root, c, chat, ct);
                        row["run"] = run;
                        foreach (var (gate, verdict) in row["gates"]!.AsObject())
                        {
                            if (verdict!.GetValue<string>().StartsWith("fail", StringComparison.Ordinal))
                            {
                                gateFailures[gate] = gateFailures.GetValueOrDefault(gate) + 1;
                            }
                        }
                        Add("harness" + suffix, row);
                    }
                }
            }
        }

        var summary = new JsonObject();
        foreach (var (arm, rows) in arms.OrderBy(a => a.Key, StringComparer.Ordinal))
        {
            summary[arm] = Summarize(rows);
        }
        var comparisons = new JsonObject();
        if (Hit(summary, "harness") is { } h)
        {
            foreach (var other in new[] { "b0", "b1", "b2", "gil" })
            {
                if (Hit(summary, other) is { } o)
                {
                    comparisons[$"harness_above_{other}"] = new JsonObject
                    {
                        ["hit_at_1"] = h.Hit.Above(o.Hit),
                        ["partial"] = h.Partial.Above(o.Partial),
                    };
                }
            }
        }
        return new JsonObject
        {
            ["kind"] = "leave_one_out",
            ["settings"] = new JsonObject
            {
                ["per_stratum"] = settings.PerStratum,
                ["seed"] = settings.Seed,
                ["runs"] = settings.Runs,
                ["min_chars"] = settings.MinChars,
                ["population"] = population.Count,
                ["strata"] = new JsonArray(sample.Strata.Select(s => (JsonNode)new JsonObject { ["name"] = s.Name, ["population"] = s.Population, ["drawn"] = s.Selected.Count }).ToArray()),
            },
            ["summary"] = summary,
            ["comparisons"] = comparisons,
            ["gate_failures"] = Json.Object(gateFailures),
            ["corpus_unchanged"] = Workspace.Fingerprint(root) == fingerprint,
            ["arms"] = new JsonObject(arms.OrderBy(a => a.Key, StringComparer.Ordinal)
                .Select(a => KeyValuePair.Create(a.Key, (JsonNode?)new JsonArray(a.Value.Select(r => (JsonNode)r).ToArray())))),
        };
    }

    /// <summary>A sampled document as a labeller sees it: an opaque id, and its text with location hints removed.</summary>
    public sealed record MaskedCase(string Id, string Path, string Folder, string Text);

    /// <summary>The same sample and masking the arms use, for labelling.</summary>
    public static List<MaskedCase> MaskedCases(string root, Settings settings)
    {
        using var baseline = new Workspace(root);
        var hierarchy = Hierarchy.Read(new Folder(baseline.Root));
        var population = hierarchy.Documents
            .Where(d => hierarchy.FolderDocument(d.Folder)?.Path != d.Path)
            .Where(d => Body(d.Text).Length >= settings.MinChars)
            .ToList();
        var strata = population.GroupBy(d => StratumOf(d.Path))
            .OrderBy(g => g.Key, StringComparer.Ordinal)
            .Select(g => new PopulationStratum(g.Key, g.Select(d => d.Path).Order(StringComparer.Ordinal).ToList()))
            .ToList();
        var sample = ReviewSampler.DrawPilot(strata, settings.PerStratum, settings.Seed);
        var cases = new List<MaskedCase>();
        var i = 0;
        foreach (var path in sample.Strata.SelectMany(s => s.Selected))
        {
            var doc = hierarchy.Document(path)!;
            using var ws = new Workspace(baseline.Root);
            cases.Add(new MaskedCase($"c{++i:00}", path, doc.Folder, TakeOut(ws.Root, doc, mask: true)));
        }
        return cases;
    }

    private static string StratumOf(string path)
    {
        var i = path.IndexOf('/');
        return i < 0 ? "(top)" : path[..i];
    }

    private static string Body(string text) =>
        string.Join("\n", Paragraphs.Split(text).Where(p => Paragraphs.HeadingLevel(p) == 0));

    /// <summary>
    /// Removes <paramref name="doc"/> from the workspace. When masking, also removes what tells
    /// where it was: links to it in other documents become their plain text (a list line that is
    /// only such a link goes), and its own relative links become their text.
    /// </summary>
    private static string TakeOut(string root, Doc doc, bool mask)
    {
        var text = doc.Text;
        if (mask)
        {
            var graph = new Folder(root).Graph().Value!;
            var incoming = graph["links"]!.AsArray()
                .Where(l => l!["to"]?.GetValue<string>() == doc.Path && l["from"]!.GetValue<string>() != doc.Path)
                .GroupBy(l => l!["from"]!.GetValue<string>());
            foreach (var group in incoming)
            {
                var file = System.IO.Path.Combine(root, group.Key);
                var bytes = File.ReadAllBytes(file);
                foreach (var link in group.OrderByDescending(l => l!["range"]!["start"]!.GetValue<int>()))
                {
                    var start = link!["range"]!["start"]!.GetValue<int>();
                    var end = link["range"]!["end"]!.GetValue<int>();
                    var label = Encoding.UTF8.GetBytes(Label(link["link"]!.GetValue<string>()));
                    bytes = [.. bytes[..start], .. label, .. bytes[end..]];
                }
                var lines = Encoding.UTF8.GetString(bytes).Split('\n');
                // A list line that was only a link to the document goes.
                var kept = lines.Where((line, i) => !(group.Any(l => l!["line"]!.GetValue<int>() == i + 1) && ListOfOne().IsMatch(line))).ToList();
                File.WriteAllText(file, string.Join('\n', kept));
            }
            text = MarkdownLink().Replace(text, m => IsExternal(m.Groups["dest"].Value) ? m.Value : m.Groups["label"].Value);
            text = WikiLink().Replace(text, m => m.Groups["label"].Success ? m.Groups["label"].Value : System.IO.Path.GetFileName(m.Groups["target"].Value));
        }
        File.Delete(System.IO.Path.Combine(root, doc.Path));
        return text;
    }

    private static string Label(string link)
    {
        var md = MarkdownLink().Match(link);
        if (md.Success)
        {
            return md.Groups["label"].Value;
        }
        var wiki = WikiLink().Match(link);
        if (wiki.Success)
        {
            return wiki.Groups["label"].Success ? wiki.Groups["label"].Value : System.IO.Path.GetFileName(wiki.Groups["target"].Value);
        }
        return link;
    }

    private static bool IsExternal(string dest) => dest.Contains("://") || dest.StartsWith("mailto:", StringComparison.Ordinal) || dest.StartsWith('#');

    private static async Task<JsonObject> HarnessAsync(string root, Case c, IChatClient chat, CancellationToken ct)
    {
        using var ws = new Workspace(root);
        var folder = new Folder(ws.Root);
        var before = Workspace.Files(ws.Root);
        var clock = Stopwatch.StartNew();
        IngestResult result;
        try
        {
            result = await new Ingester(folder, chat).IngestAsync(new IngestRequest(c.Text), null, ct);
        }
        catch (Exception e) when (e is not OperationCanceledException)
        {
            var error = Row(c, null, null);
            error["outcome"] = "error";
            error["error"] = e.GetType().Name + ": " + e.Message;
            error["gates"] = new JsonObject();
            error["model_calls"] = 0;
            error["tokens"] = 0L;
            return error;
        }
        var seconds = clock.Elapsed.TotalSeconds;
        var applied = result.Plan is not null && folder.Apply(result.Plan).Ok;
        var after = Workspace.Files(ws.Root);
        var gates = Gates.Judge(before, after, c.Text, result, [], applied, null);
        // Where the largest piece landed.
        var main = result.Placement
            .GroupBy(p => p.Piece)
            .Select(g => g.First())
            .OrderBy(p => p.Piece)
            .FirstOrDefault();
        var landing = main is null ? null : LandingOf(main);
        var row = Row(c, landing, main?.Form);
        row["outcome"] = result.Outcome;
        row["pieces"] = result.Placement.Select(p => p.Piece).Distinct().Count();
        row["target"] = main?.Target;
        row["gates"] = Json.Object(gates);
        row["model_calls"] = result.Trace.ModelCalls;
        row["tokens"] = result.Trace.InputTokens + result.Trace.OutputTokens;
        row["repairs"] = result.Trace.Repairs;
        row["violations"] = result.Violations is null ? null : new JsonArray(result.Violations.Select(v => (JsonNode)v).ToArray());
        row["unseen_refused"] = result.Trace.UnseenCitations;
        row["path_taken"] = string.Join(" > ", result.Trace.Steps.Where(s => s.Refusal is null).Select(s => s.At.Length == 0 ? "(top)" : s.At));
        row["machine"] = new JsonObject { ["seconds"] = Math.Round(seconds, 2) };
        return row;
    }

    /// <summary>The folder a placement puts the text in: the folder of the document written, or where a new folder was made.</summary>
    public static string LandingOf(Placement p) =>
        p.Form == "add_folder" ? Hierarchy.DirOf(Hierarchy.DirOf(p.Target)) : Hierarchy.DirOf(p.Target);

    public static JsonObject Row(string path, string folder, string? landing, string? form)
    {
        var hit = landing is not null && Same(landing, folder);
        return new JsonObject
        {
            ["doc"] = path,
            ["truth"] = folder,
            ["landing"] = landing,
            ["form"] = form,
            ["hit"] = hit,
            ["partial"] = landing is null ? 0 : Partial(landing, folder),
        };
    }

    private static JsonObject Row(Case c, string? landing, string? form) => Row(c.Path, c.Folder, landing, form);

    private static bool Same(string a, string b) =>
        string.Equals(a.Normalize(NormalizationForm.FormC), b.Normalize(NormalizationForm.FormC), StringComparison.Ordinal);

    /// <summary>The depth of the deepest common folder over the depth of the true folder (1 when the true folder is the top and it landed there).</summary>
    public static double Partial(string landing, string truth)
    {
        var t = Segments(truth);
        var l = Segments(landing);
        if (t.Length == 0)
        {
            return l.Length == 0 ? 1 : 0;
        }
        var common = 0;
        while (common < t.Length && common < l.Length && t[common] == l[common])
        {
            common++;
        }
        return Math.Round((double)common / Math.Max(t.Length, l.Length), 4);
    }

    private static string[] Segments(string folder) =>
        folder.Normalize(NormalizationForm.FormC).Split('/', StringSplitOptions.RemoveEmptyEntries);

    private static JsonObject Summarize(List<JsonObject> rows)
    {
        var hits = rows.Count(r => r["hit"]!.GetValue<bool>());
        var partial = rows.Select(r => r["partial"]!.GetValue<double>()).ToList();
        var o = new JsonObject
        {
            ["hit_at_1"] = Json.Of(Proportion.Of(hits, rows.Count)),
            ["partial"] = Json.Of(Mean.Of(partial)),
        };
        var forms = rows.Where(r => r["form"] is not null).GroupBy(r => r["form"]!.GetValue<string>()).OrderBy(g => g.Key, StringComparer.Ordinal);
        if (forms.Any())
        {
            o["forms"] = new JsonObject(forms.Select(g => KeyValuePair.Create(g.Key, (JsonNode?)g.Count())));
        }
        if (rows.Any(r => r["outcome"] is not null))
        {
            o["outcomes"] = new JsonObject(rows.GroupBy(r => r["outcome"]!.GetValue<string>()).OrderBy(g => g.Key, StringComparer.Ordinal)
                .Select(g => KeyValuePair.Create(g.Key, (JsonNode?)g.Count())));
        }
        if (rows.Any(r => r["model_calls"] is not null))
        {
            o["model_calls_median"] = Median.Of(rows.Select(r => (double)r["model_calls"]!.GetValue<int>()));
            o["tokens_median"] = Median.Of(rows.Select(r => (double)r["tokens"]!.GetValue<long>()));
        }
        if (rows.Any(r => r["machine"]?["seconds"] is not null))
        {
            o["seconds_median"] = Math.Round(Median.Of(rows.Select(r => r["machine"]!["seconds"]!.GetValue<double>())), 2);
        }
        if (rows.Any(r => r["answered"] is not null))
        {
            var answered = rows.Where(r => r["answered"]!.GetValue<bool>()).ToList();
            o["answered"] = Json.Of(Proportion.Of(answered.Count, rows.Count));
            o["precision_when_answered"] = Json.Of(Proportion.Of(answered.Count(r => r["hit"]!.GetValue<bool>()), answered.Count));
            o["first_candidate_hit"] = Json.Of(Proportion.Of(rows.Count(r => r["first_candidate_hit"]?.GetValue<bool>() == true), rows.Count));
        }
        return o;
    }

    private static (Proportion Hit, Mean Partial)? Hit(JsonObject summary, string arm)
    {
        if (summary[arm] is not JsonObject a)
        {
            return null;
        }
        var h = a["hit_at_1"]!;
        var p = a["partial"]!;
        double D(JsonNode? n) => n?.GetValue<double>() ?? double.NaN;
        return (new Proportion(D(h["value"]), D(h["low"]), D(h["high"]), h["successes"]!.GetValue<int>(), h["n"]!.GetValue<int>()),
                new Mean(D(p["value"]), D(p["low"]), D(p["high"]), p["n"]!.GetValue<int>()));
    }

    [GeneratedRegex(@"!?\[(?<label>[^\]]*)\]\((?<dest>[^)\s]*)(?:\s+""[^""]*"")?\)")]
    private static partial Regex MarkdownLink();

    [GeneratedRegex(@"\[\[(?<target>[^\]|#]*)(?:#[^\]|]*)?(?:\|(?<label>[^\]]*))?\]\]")]
    private static partial Regex WikiLink();

    [GeneratedRegex(@"^\s*[-*+]\s+\S.{0,120}$")]
    private static partial Regex ListOfOne();
}
