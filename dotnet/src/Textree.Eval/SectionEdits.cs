using System.Text;
using System.Text.Json.Nodes;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// Section-edit preservation (quality.md §4). Its measure is the calibrated judge's claim-level
/// recall (§7); what is measured here is the deterministic floor under it — every paragraph the
/// edited document had before is still there, word for word, after the plan is applied.
/// </summary>
public static class SectionEdits
{
    /// <summary>How many documents the plan edits a section of, and in how many every earlier paragraph survived.</summary>
    public static (int Edits, int Kept) Measure(JsonNode? plan, SortedDictionary<string, byte[]> before, SortedDictionary<string, byte[]> after, bool applied)
    {
        var edited = plan?["ops"]?.AsArray().Where(o => o!["op"]!.GetValue<string>() == "edit_section")
            .Select(o => o!["path"]!.GetValue<string>()).Distinct(StringComparer.Ordinal).ToList() ?? [];
        var kept = edited.Count(path =>
            applied && before.TryGetValue(path, out var was) && after.TryGetValue(path, out var now)
            && Paragraphs.Keys(Encoding.UTF8.GetString(was)).IsSubsetOf(Paragraphs.Keys(Encoding.UTF8.GetString(now))));
        return (edited.Count, kept);
    }

    /// <summary>The report entry over rows carrying <c>section_edits</c> and <c>section_edits_kept</c>.</summary>
    public static JsonObject Summarize(IEnumerable<JsonNode?> rows, string whenNone)
    {
        var list = rows.OfType<JsonObject>().ToList();
        var edits = list.Sum(r => r["section_edits"]?.GetValue<int>() ?? 0);
        var kept = list.Sum(r => r["section_edits_kept"]?.GetValue<int>() ?? 0);
        return edits == 0
            ? new JsonObject { ["section_edits"] = 0, ["value"] = "n/a", ["note"] = whenNone }
            : new JsonObject
            {
                ["section_edits"] = edits,
                ["paragraphs_kept_verbatim"] = Json.Of(Proportion.Of(kept, edits)),
                ["claim_recall"] = "the calibrated judge's (quality.md §7) — not measured until the judge passes calibration",
                ["note"] = "each edited document's earlier paragraphs all kept word for word",
            };
    }
}
