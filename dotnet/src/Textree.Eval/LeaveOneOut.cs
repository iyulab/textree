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
    /// <param name="Present">Also ingest each drawn document's text with the document left in place: nothing is the right answer (no-action accuracy).</param>
    public sealed record Settings(int PerStratum, ulong Seed, int Runs, int MinChars, bool Unmasked, IReadOnlySet<string> Arms, string? Only = null, bool Present = false);

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
            var conditions = new List<string> { "masked" };
            if (settings.Unmasked)
            {
                conditions.Add("unmasked");
            }
            if (settings.Present)
            {
                conditions.Add("present");
            }
            foreach (var condition in conditions)
            {
                using var ws = new Workspace(baseline.Root);
                // Present: the document stays where it is and its own text comes in again.
                var text = condition == "present" ? doc.Text : TakeOut(ws.Root, doc, condition == "masked");
                var c = new Case(path, doc.Folder, stratum, text);
                var remaining = Hierarchy.Read(new Folder(ws.Root));
                var suffix = condition == "masked" ? "" : "." + condition;
                JsonObject Mark(JsonObject row)
                {
                    if (condition == "present")
                    {
                        // A baseline that names a folder always acts; only an abstention or an empty plan is no action.
                        row["no_action"] = row["outcome"]?.GetValue<string>() == "empty" || row["answered"]?.GetValue<bool>() == false;
                    }
                    return row;
                }

                JsonObject Timed(Func<JsonObject> arm)
                {
                    var start = Clock.Start(chat);
                    var row = arm();
                    row["machine"] = Clock.Machine(chat, start);
                    return row;
                }
                if (settings.Arms.Contains("b0"))
                {
                    Add("b0" + suffix, Mark(Timed(() => Row(c, "", null))));
                }
                if (settings.Arms.Contains("b1"))
                {
                    Add("b1" + suffix, Mark(Timed(() => Row(c, Baselines.Nearest(remaining, text), null))));
                }
                if (settings.Arms.Contains("gil"))
                {
                    Add("gil" + suffix, Mark(Timed(() => Baselines.Gil(remaining, c.Path, text))));
                }
                for (var run = 1; run <= settings.Runs; run++)
                {
                    if (settings.Arms.Contains("b2"))
                    {
                        var start = Clock.Start(chat);
                        var (landing, calls, tokens) = await Baselines.SingleShotAsync(remaining, text, chat, ct);
                        var row = Row(c, landing, null);
                        row["run"] = run;
                        row["model_calls"] = calls;
                        row["tokens"] = tokens;
                        row["machine"] = Clock.Machine(chat, start);
                        Add("b2" + suffix, Mark(row));
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
                        Add("harness" + suffix, Mark(row));
                    }
                }
            }
        }

        var summary = new JsonObject();
        foreach (var (arm, rows) in arms.OrderBy(a => a.Key, StringComparer.Ordinal))
        {
            summary[arm] = Summarize(rows);
        }
        // Each comparison twice: whether the intervals are apart, and the difference on the same
        // documents (paired, the document as the unit) with its interval.
        var comparisons = new JsonObject();
        if (arms.TryGetValue("harness", out var harnessRows))
        {
            foreach (var other in new[] { "b0", "b1", "b2", "gil" })
            {
                if (arms.TryGetValue(other, out var otherRows))
                {
                    var h = Interval(summary, "harness");
                    var o = Interval(summary, other);
                    comparisons[$"harness_above_{other}"] = new JsonObject
                    {
                        ["hit_at_1"] = h.HitLow > o.HitHigh,
                        ["partial"] = h.PartialLow > o.PartialHigh,
                        ["paired_hit_difference"] = Json.Of(Paired(harnessRows, otherRows, "hit")),
                        ["paired_partial_difference"] = Json.Of(Paired(harnessRows, otherRows, "partial")),
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
        var start = Clock.Start(chat);
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
        var machine = Clock.Machine(chat, start);
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
        // Section edits: does every paragraph the document had before survive the edit, word for word?
        var edited = result.Plan?["ops"]?.AsArray().Where(o => o!["op"]!.GetValue<string>() == "edit_section")
            .Select(o => o!["path"]!.GetValue<string>()).Distinct(StringComparer.Ordinal).ToList() ?? [];
        if (edited.Count > 0)
        {
            row["section_edits"] = edited.Count;
            row["section_edits_kept"] = edited.Count(path =>
                applied && before.TryGetValue(path, out var was) && after.TryGetValue(path, out var now)
                && Paragraphs.Keys(Encoding.UTF8.GetString(was)).IsSubsetOf(Paragraphs.Keys(Encoding.UTF8.GetString(now))));
        }
        row["path_taken"] = string.Join(" > ", result.Trace.Steps.Where(s => s.Refusal is null).Select(s => s.At.Length == 0 ? "(top)" : s.At));
        row["machine"] = machine;
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
        // The document is the unit: repeated runs of one document are not independent trials.
        var hit = ByDocument(rows, "hit");
        var partial = ByDocument(rows, "partial");
        var runs = rows.GroupBy(r => r["doc"]!.GetValue<string>(), StringComparer.Ordinal).Max(g => g.Count());
        var o = new JsonObject
        {
            ["documents"] = hit.Count,
            ["runs"] = runs,
            ["hit_at_1"] = runs == 1
                ? Json.Of(Proportion.Of(hit.Values.Count(v => v == 1), hit.Count))
                : Json.Of(Mean.Of(hit.Values.ToList())),
            ["partial"] = Json.Of(Mean.Of(partial.Values.ToList())),
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
        // Form: in leave-one-out the text was a whole document, so the expected form is a new
        // document. Arms that only name a folder have no form of their own.
        if (rows.Any(r => r["form"] is not null))
        {
            o["form_accuracy"] = FormAccuracy(rows);
        }
        else
        {
            o["form_accuracy"] = rows.Any(r => r["outcome"] is not null)
                ? "n/a: no placement made (every outcome is empty or failed)"
                : "n/a: the arm names a folder only";
        }
        if (rows.Any(r => r["no_action"] is not null))
        {
            o["no_action"] = Json.Of(Proportion.Of(rows.Count(r => r["no_action"]!.GetValue<bool>()), rows.Count));
        }
        // Section-edit preservation: claim-level recall is the judge's (quality.md §7). What is
        // measured here is the deterministic floor — every paragraph of the edited document kept verbatim.
        var sectionEdits = rows.Sum(r => r["section_edits"]?.GetValue<int>() ?? 0);
        var kept = rows.Sum(r => r["section_edits_kept"]?.GetValue<int>() ?? 0);
        o["section_edit_preservation"] = sectionEdits == 0
            ? new JsonObject { ["section_edits"] = 0, ["value"] = null, ["note"] = "no section edits: nothing to preserve" }
            : new JsonObject
            {
                ["section_edits"] = sectionEdits,
                ["paragraphs_kept_verbatim"] = Json.Of(Proportion.Of(kept, sectionEdits)),
                ["note"] = "each edited document's earlier paragraphs all kept word for word; claim-level recall is the calibrated judge's (quality.md §7)",
            };
        // Cost, per placement: median with its interval and N.
        var cost = new JsonObject();
        if (rows.Any(r => r["model_calls"] is not null))
        {
            o["model_calls_median"] = Median.Of(rows.Select(r => (double)r["model_calls"]!.GetValue<int>()));
            o["tokens_median"] = Median.Of(rows.Select(r => (double)r["tokens"]!.GetValue<long>()));
            cost["model_calls"] = Json.Of(MedianInterval.Of(rows.Select(r => (double)r["model_calls"]!.GetValue<int>())));
            cost["tokens"] = Json.Of(MedianInterval.Of(rows.Select(r => (double)r["tokens"]!.GetValue<long>())));
        }
        else
        {
            cost["model_calls"] = Json.Of(MedianInterval.Of(rows.Select(_ => 0.0)));
            cost["tokens"] = Json.Of(MedianInterval.Of(rows.Select(_ => 0.0)));
        }
        if (rows.Any(r => r["machine"]?["seconds"] is not null))
        {
            o["seconds_median"] = Math.Round(Median.Of(rows.Select(r => r["machine"]!["seconds"]!.GetValue<double>())), 2);
            o["seconds_clock"] = new JsonArray(rows.Select(r => r["machine"]?["clock"]?.GetValue<string>()).OfType<string>()
                .Distinct().Order(StringComparer.Ordinal).Select(x => (JsonNode)x).ToArray());
            cost["seconds"] = Json.Of(MedianInterval.Of(rows.Where(r => r["machine"]?["seconds"] is not null).Select(r => r["machine"]!["seconds"]!.GetValue<double>())));
        }
        o["cost"] = cost;
        if (rows.Any(r => r["answered"] is not null))
        {
            var answered = rows.Where(r => r["answered"]!.GetValue<bool>()).ToList();
            o["answered"] = Json.Of(Proportion.Of(answered.Count, rows.Count));
            o["precision_when_answered"] = Json.Of(Proportion.Of(answered.Count(r => r["hit"]!.GetValue<bool>()), answered.Count));
            o["first_candidate_hit"] = Json.Of(Proportion.Of(rows.Count(r => r["first_candidate_hit"]?.GetValue<bool>() == true), rows.Count));
        }
        return o;
    }

    /// <summary>Macro F1 over the forms the cases expect (in leave-one-out only a new document, so it is
    /// that form's F1), with the counts; the interval is on the share placed as expected.</summary>
    private static JsonObject FormAccuracy(List<JsonObject> rows)
    {
        const string expected = "add_document";
        var found = rows.Select(r => r["form"]?.GetValue<string>()).ToList();
        var tp = found.Count(f => f == expected);
        // Precision of the expected form: of the placements made in that form, how many were expected.
        var predicted = found.Count(f => f == expected);
        var precision = predicted == 0 ? 0 : (double)tp / predicted;
        var recall = rows.Count == 0 ? 0 : (double)tp / rows.Count;
        var f1 = precision + recall == 0 ? 0 : 2 * precision * recall / (precision + recall);
        return new JsonObject
        {
            ["macro_f1"] = Math.Round(f1, 4),
            ["expected"] = new JsonObject { [expected] = rows.Count },
            ["found"] = new JsonObject(found.GroupBy(f => f ?? "(none)").OrderBy(g => g.Key, StringComparer.Ordinal)
                .Select(g => KeyValuePair.Create(g.Key, (JsonNode?)g.Count()))),
            ["share_as_expected"] = Json.Of(Proportion.Of(tp, rows.Count)),
        };
    }

    private static (double HitLow, double HitHigh, double PartialLow, double PartialHigh) Interval(JsonObject summary, string arm)
    {
        double D(JsonNode? n) => n?.GetValue<double>() ?? double.NaN;
        var a = summary[arm]!;
        return (D(a["hit_at_1"]!["low"]), D(a["hit_at_1"]!["high"]), D(a["partial"]!["low"]), D(a["partial"]!["high"]));
    }

    /// <summary>A row's score: 1 or 0 for a hit, else the partial score.</summary>
    private static double Score(JsonObject row, string measure) =>
        measure == "hit" ? (row["hit"]!.GetValue<bool>() ? 1 : 0) : row["partial"]!.GetValue<double>();

    /// <summary>Each document's score, averaged over its runs.</summary>
    private static Dictionary<string, double> ByDocument(List<JsonObject> rows, string measure) =>
        rows.GroupBy(r => r["doc"]!.GetValue<string>(), StringComparer.Ordinal).ToDictionary(g => g.Key, g => g.Average(r => Score(r, measure)), StringComparer.Ordinal);

    /// <summary>The mean difference a − b over the documents both arms placed.</summary>
    private static Mean Paired(List<JsonObject> a, List<JsonObject> b, string measure)
    {
        var da = ByDocument(a, measure);
        var db = ByDocument(b, measure);
        return Mean.Of(da.Keys.Where(db.ContainsKey).Order(StringComparer.Ordinal).Select(k => da[k] - db[k]).ToList());
    }

    [GeneratedRegex(@"!?\[(?<label>[^\]]*)\]\((?<dest>[^)\s]*)(?:\s+""[^""]*"")?\)")]
    private static partial Regex MarkdownLink();

    [GeneratedRegex(@"\[\[(?<target>[^\]|#]*)(?:#[^\]|]*)?(?:\|(?<label>[^\]]*))?\]\]")]
    private static partial Regex WikiLink();

    [GeneratedRegex(@"^\s*[-*+]\s+\S.{0,120}$")]
    private static partial Regex ListOfOne();
}
