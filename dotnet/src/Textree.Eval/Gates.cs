using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// The absolute gates of an ingest (quality.md §3), judged from the folder before and after the
/// plan is applied, the input and the harness's result. Each gate is "pass", "fail: …" or "n/a".
/// </summary>
public static partial class Gates
{
    public const string Pass = "pass";
    public const string NotApplicable = "n/a";

    /// <param name="untouched">Path prefixes nothing may change under (none: G5 is not applicable).</param>
    /// <param name="applied">Whether the plan was applied (false: refused, or there was none).</param>
    /// <param name="reingestEmpty">Whether ingesting the same text again gave an empty outcome (null: not tried).</param>
    public static SortedDictionary<string, string> Judge(
        SortedDictionary<string, byte[]> before,
        SortedDictionary<string, byte[]> after,
        string input,
        IngestResult result,
        IReadOnlyList<string> untouched,
        bool applied,
        bool? reingestEmpty)
    {
        var g = new SortedDictionary<string, string>(StringComparer.Ordinal);
        var beforeText = Texts(before);
        var afterText = Texts(after);
        var beforeKeys = beforeText.Values.SelectMany(t => Paragraphs.Split(t)).Select(Paragraphs.Key).ToHashSet();
        var afterKeys = afterText.Values.SelectMany(t => Paragraphs.Split(t)).Select(Paragraphs.Key).ToHashSet();
        var inputParagraphs = Paragraphs.Split(input);

        // G3: a plan the harness returns is one the engine applies; a failure returns no plan.
        g["G3"] = result.Outcome switch
        {
            "planned" when !applied => "fail: the returned plan was refused when applied",
            "failed" when result.Plan is not null => "fail: a failed outcome carries a plan",
            _ => Pass,
        };

        if (result.Outcome != "planned")
        {
            var unchanged = before.Count == after.Count && before.All(kv => after.TryGetValue(kv.Key, out var b) && b.AsSpan().SequenceEqual(kv.Value));
            var missing = inputParagraphs.Where(p => !beforeKeys.Contains(Paragraphs.Key(p)) && Paragraphs.HeadingLevel(p) == 0).ToList();
            g["G1"] = result.Outcome == "empty" && missing.Count > 0 ? $"fail: empty outcome but {missing.Count} paragraph(s) are not in the folder" : (result.Outcome == "empty" ? Pass : NotApplicable);
            g["G2"] = unchanged ? Pass : "fail: the folder changed without a plan";
            foreach (var k in new[] { "G4", "G5", "G6", "G7", "G8", "G9" })
            {
                g[k] = NotApplicable;
            }
            if (result.Outcome == "empty")
            {
                g["G4"] = Pass;
            }
            return g;
        }

        // G1: every paragraph of the input is in the folder after.
        var lost = inputParagraphs.Where(p => Paragraphs.HeadingLevel(p) == 0 && !afterKeys.Contains(Paragraphs.Key(p))).ToList();
        g["G1"] = lost.Count == 0 ? Pass : $"fail: {lost.Count} input paragraph(s) missing, first: {Clip(lost[0])}";

        // G2: every paragraph that was in the folder is still there.
        var gone = beforeText.Values.SelectMany(t => Paragraphs.Split(t)).Where(p => !afterKeys.Contains(Paragraphs.Key(p))).ToList();
        g["G2"] = gone.Count == 0 ? Pass : $"fail: {gone.Count} existing paragraph(s) lost, first: {Clip(gone[0])}";

        // G4: ingesting the same text again gives nothing to do.
        g["G4"] = reingestEmpty switch { null => NotApplicable, true => Pass, false => "fail: the same text again gave a plan" };

        // G5: nothing changed under the untouched paths.
        var touchedUntouched = Changed(before, after).Where(p => untouched.Any(u => Under(p, u))).ToList();
        g["G5"] = untouched.Count == 0 ? NotApplicable
            : touchedUntouched.Count == 0 ? Pass
            : $"fail: changed {string.Join(", ", touchedUntouched)}";

        // G6: placing only writes text — no file removed or renamed, and existing documents only grow.
        var removed = before.Keys.Where(k => !after.ContainsKey(k)).ToList();
        var ops = result.Plan?["ops"]?.AsArray().Select(o => o!["op"]!.GetValue<string>()).ToList() ?? [];
        var structural = ops.Where(o => o is not ("add" or "append" or "edit_section")).ToList();
        g["G6"] = removed.Count > 0 ? $"fail: removed {string.Join(", ", removed)}"
            : structural.Count > 0 ? $"fail: operations {string.Join(", ", structural)}"
            : Pass;

        // G7: text in the folder that is new and is not from the input is only headings and link lines.
        var inputKeys = inputParagraphs.Select(Paragraphs.Key).ToHashSet();
        var invented = afterText.Values.SelectMany(t => Paragraphs.Split(t))
            .Where(p => !beforeKeys.Contains(Paragraphs.Key(p)) && !inputKeys.Contains(Paragraphs.Key(p)))
            .Where(p => Paragraphs.HeadingLevel(p) == 0 && !IsLinkList(p))
            .ToList();
        g["G7"] = invented.Count == 0 ? Pass : $"fail: {invented.Count} paragraph(s) neither in the folder nor in the input, first: {Clip(invented[0])}";

        // G8: nothing outside the placement targets (and the folder documents that list new ones) changed.
        var targets = result.Placement.Select(p => p.Target).ToHashSet(StringComparer.Ordinal);
        var stray = Changed(before, after).Where(p => !targets.Contains(p) && !ListsANewTarget(p, targets, after)).ToList();
        g["G8"] = stray.Count == 0 ? Pass : $"fail: changed outside the placement: {string.Join(", ", stray)}";

        // G9: every place is one the model was shown — an existing document listed in a step, or a new one in a folder it was in.
        var shown = result.Trace.Steps.SelectMany(s => s.Shown).ToHashSet(StringComparer.Ordinal);
        var visited = result.Trace.Steps.Select(s => s.At).Append("").ToHashSet(StringComparer.Ordinal);
        var unseen = result.Placement.Where(p =>
            p.Form is "append_section" or "add_to_section" ? !shown.Contains(p.Target)
            : p.Form == "add_document" ? !visited.Contains(Hierarchy.DirOf(p.Target))
            : !visited.Contains(Hierarchy.DirOf(Hierarchy.DirOf(p.Target)))).ToList();
        g["G9"] = unseen.Count == 0 ? Pass : $"fail: placed where it was not shown: {string.Join(", ", unseen.Select(p => p.Target))}";
        return g;
    }

    /// <summary>Paths whose bytes differ (or that exist on one side only).</summary>
    public static List<string> Changed(SortedDictionary<string, byte[]> before, SortedDictionary<string, byte[]> after) =>
        before.Keys.Union(after.Keys)
            .Where(k => !(before.TryGetValue(k, out var a) && after.TryGetValue(k, out var b) && a.AsSpan().SequenceEqual(b)))
            .Order(StringComparer.Ordinal)
            .ToList();

    /// <summary>Whether a path is <paramref name="prefix"/> or under it (Unicode normalization forms compare equal).</summary>
    public static bool Under(string path, string prefix)
    {
        path = path.Normalize(NormalizationForm.FormC);
        prefix = prefix.Normalize(NormalizationForm.FormC);
        return prefix.Length == 0 || path == prefix.TrimEnd('/') || path.StartsWith(prefix.TrimEnd('/') + "/", StringComparison.Ordinal)
            || (prefix.EndsWith('*') && path.StartsWith(prefix.TrimEnd('*'), StringComparison.Ordinal));
    }

    private static bool ListsANewTarget(string path, HashSet<string> targets, SortedDictionary<string, byte[]> after)
    {
        // The folder document of a folder a new document or folder went into.
        var dir = Hierarchy.DirOf(path);
        return Hierarchy.NameOf(path) == Hierarchy.NameOf(dir) + ".md"
            && targets.Any(t => Hierarchy.DirOf(t) == dir || Hierarchy.DirOf(Hierarchy.DirOf(t)) == dir);
    }

    private static bool IsLinkList(string paragraph) =>
        paragraph.Split('\n', StringSplitOptions.RemoveEmptyEntries).All(l => LinkLine().IsMatch(l.Trim()));

    private static Dictionary<string, string> Texts(SortedDictionary<string, byte[]> files) =>
        files.Where(kv => kv.Key.EndsWith(".md", StringComparison.OrdinalIgnoreCase))
            .ToDictionary(kv => kv.Key, kv => Encoding.UTF8.GetString(kv.Value));

    private static string Clip(string s) => s.Length <= 80 ? s.Replace('\n', ' ') : s[..80].Replace('\n', ' ') + "…";

    [GeneratedRegex(@"^[-*] \[[^\]]*\]\([^)]*\)$")]
    private static partial Regex LinkLine();
}
