using System.Text.Json.Nodes;
using Textree.Eval;
using Xunit;

namespace Textree.Eval.Tests;

public class ComparisonsTests
{
    private static JsonObject Row(string doc, bool hit, double partial) =>
        new() { ["doc"] = doc, ["hit"] = hit, ["partial"] = partial };

    private static JsonObject Arm(double hitLow, double hitHigh, double partialLow, double partialHigh) => new()
    {
        ["hit_at_1"] = new JsonObject { ["low"] = hitLow, ["high"] = hitHigh },
        ["partial"] = new JsonObject { ["low"] = partialLow, ["high"] = partialHigh },
    };

    [Fact]
    public void The_harness_is_above_when_the_paired_difference_clears_zero_though_the_intervals_overlap()
    {
        // The harness beats the baseline on every one of the same documents by a small margin:
        // each arm's own interval is wide (the documents differ a lot), the difference is not.
        var docs = Enumerable.Range(1, 12).Select(i => $"d{i}.md").ToList();
        var harness = docs.Select((d, i) => Row(d, i % 2 == 0, 0.30 + 0.05 * i)).ToList();
        var baseline = docs.Select((d, i) => Row(d, false, 0.25 + 0.05 * i)).ToList();
        var summary = new JsonObject { ["harness"] = Arm(0.2, 0.8, 0.4, 0.7), ["b1"] = Arm(0.0, 0.3, 0.35, 0.65) };

        var c = LeaveOneOut.Comparisons(summary, new Dictionary<string, List<JsonObject>> { ["harness"] = harness, ["b1"] = baseline });

        Assert.True(c["harness_above_b1"]!["partial"]!.GetValue<bool>());
        Assert.False(c["intervals_apart_b1"]!["partial"]!.GetValue<bool>());
        Assert.False(c["intervals_apart_b1"]!["hit_at_1"]!.GetValue<bool>());
    }

    [Fact]
    public void A_difference_whose_interval_reaches_zero_is_not_above()
    {
        var harness = new List<JsonObject> { Row("a.md", true, 1), Row("b.md", false, 0) };
        var baseline = new List<JsonObject> { Row("a.md", false, 0), Row("b.md", true, 1) };
        var summary = new JsonObject { ["harness"] = Arm(0, 1, 0, 1), ["b2"] = Arm(0, 1, 0, 1) };

        var c = LeaveOneOut.Comparisons(summary, new Dictionary<string, List<JsonObject>> { ["harness"] = harness, ["b2"] = baseline });

        Assert.False(c["harness_above_b2"]!["hit_at_1"]!.GetValue<bool>());
        Assert.False(c["harness_above_b2"]!["partial"]!.GetValue<bool>());
    }

    [Fact]
    public void No_documents_in_common_is_not_above()
    {
        var c = LeaveOneOut.Comparisons(
            new JsonObject { ["harness"] = Arm(0.9, 1, 0.9, 1), ["b0"] = Arm(0, 0.1, 0, 0.1) },
            new Dictionary<string, List<JsonObject>> { ["harness"] = [Row("a.md", true, 1)], ["b0"] = [Row("b.md", false, 0)] });

        Assert.False(c["harness_above_b0"]!["hit_at_1"]!.GetValue<bool>());
        Assert.True(c["intervals_apart_b0"]!["hit_at_1"]!.GetValue<bool>());
    }
}
