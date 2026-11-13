using System.Text.Json;
using System.Text.Json.Nodes;
using Textree.Eval;
using Xunit;

namespace Textree.Eval.Tests;

public sealed class ReplayTests
{
    private static string Corpus()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
        {
            var candidate = Path.Combine(dir.FullName, "tests", "data", "ingest");
            if (File.Exists(Path.Combine(candidate, "recorded-answers.json")))
            {
                return candidate;
            }
        }
        throw new DirectoryNotFoundException("tests/data/ingest");
    }

    /// <summary>The report without what differs from machine to machine (time taken).</summary>
    private static string WithoutMachine(JsonObject report)
    {
        static void Strip(JsonNode? node)
        {
            switch (node)
            {
                case JsonObject o:
                    o.Remove("machine");
                    foreach (var (_, v) in o.ToList())
                    {
                        Strip(v);
                    }
                    break;
                case JsonArray a:
                    foreach (var v in a)
                    {
                        Strip(v);
                    }
                    break;
            }
        }
        var copy = report.DeepClone().AsObject();
        Strip(copy);
        return copy.ToJsonString(new JsonSerializerOptions { WriteIndented = true });
    }

    [Fact]
    public async Task Replaying_recorded_answers_gives_the_same_report_twice_and_no_gate_fails()
    {
        var corpus = Corpus();
        var recording = Path.Combine(corpus, "recorded-answers.json");
        var ct = TestContext.Current.CancellationToken;
        var first = await Cases.RunAsync(corpus, RecordingChatClient.Replay(recording), 1, ct);
        var second = await Cases.RunAsync(corpus, RecordingChatClient.Replay(recording), 1, ct);
        Assert.Equal(WithoutMachine(first), WithoutMachine(second));
        Assert.Empty(first["summary"]!["gate_failures"]!.AsObject());
        Assert.Equal(10, first["summary"]!["cases"]!.GetValue<int>());
    }
}
