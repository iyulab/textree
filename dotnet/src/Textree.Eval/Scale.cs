using System.Text;
using System.Text.Json.Nodes;
using Microsoft.Extensions.AI;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// The cost of one ingest as the tree grows: synthetic trees of 100, 1,000 and 10,000 documents
/// (ten folders per folder, ten documents per folder at the bottom), the same texts ingested into
/// each. Walking from the top should cost about one model call per level, not per document.
/// </summary>
public static class Scale
{
    private static readonly string[] Areas = ["garden", "kitchen", "workshop", "travel", "health", "money", "reading", "music", "family", "work"];
    private static readonly string[] Facets = ["planning", "tools", "costs", "people", "schedule", "problems", "ideas", "log", "sources", "lessons"];
    private static readonly string[] Items = ["north", "south", "spring", "autumn", "small", "large", "first", "second", "old", "new"];

    public static async Task<JsonObject> RunAsync(IChatClient chat, IReadOnlyList<int> sizes, CancellationToken ct = default)
    {
        var texts = Areas.Select((a, i) =>
            $"Note about {a} {Facets[i]}: the {Items[i]} part needs another look next week. It cost more than planned and took two afternoons.").ToList();
        var rows = new JsonArray();
        var medians = new JsonObject();
        foreach (var size in sizes)
        {
            var generated = Generate(size);
            using var ws = new Workspace(generated);
            Directory.Delete(generated, recursive: true);
            var folder = new Folder(ws.Root);
            var calls = new List<double>();
            var tokens = new List<double>();
            var seconds = new List<double>();
            foreach (var text in texts)
            {
                var clock = System.Diagnostics.Stopwatch.StartNew();
                var r = await new Ingester(folder, chat).IngestAsync(new IngestRequest(text), null, ct);
                calls.Add(r.Trace.ModelCalls);
                tokens.Add(r.Trace.InputTokens + r.Trace.OutputTokens);
                seconds.Add(clock.Elapsed.TotalSeconds);
                rows.Add(new JsonObject { ["size"] = size, ["outcome"] = r.Outcome, ["model_calls"] = r.Trace.ModelCalls, ["depth"] = r.Trace.Steps.Count(s => s.Refusal is null), ["tokens"] = r.Trace.InputTokens + r.Trace.OutputTokens });
            }
            medians[size.ToString()] = new JsonObject
            {
                ["model_calls_median"] = Median.Of(calls),
                ["tokens_median"] = Median.Of(tokens),
                ["machine"] = new JsonObject { ["seconds_median"] = Math.Round(Median.Of(seconds), 2) },
            };
        }
        var smallest = medians[sizes.Min().ToString()]!["model_calls_median"]!.GetValue<double>();
        var largest = medians[sizes.Max().ToString()]!["model_calls_median"]!.GetValue<double>();
        return new JsonObject
        {
            ["kind"] = "scale",
            ["sizes"] = medians,
            ["calls_ratio_largest_to_smallest"] = Math.Round(largest / smallest, 3),
            ["rows"] = rows,
        };
    }

    /// <summary>A synthetic tree of about <paramref name="documents"/> documents, in the temporary directory.</summary>
    public static string Generate(int documents)
    {
        var root = Path.Combine(Path.GetTempPath(), $"textree-scale-{documents}-" + Guid.NewGuid().ToString("N")[..8]);
        var levels = documents switch { <= 100 => 0, <= 1000 => 1, _ => 2 };
        void Make(string dir, int level, List<string> trail)
        {
            Directory.CreateDirectory(dir);
            var name = Path.GetFileName(dir);
            var words = level == 0 ? Areas : level == 1 ? Facets : Items;
            if (level < levels)
            {
                var own = new StringBuilder($"# {Title(name)}\n\nNotes on {string.Join(" ", trail)}.\n");
                File.WriteAllText(Path.Combine(dir, name + ".md"), own.ToString());
                foreach (var w in words)
                {
                    Make(Path.Combine(dir, w), level + 1, [.. trail, w]);
                }
                return;
            }
            for (var i = 1; i <= 10; i++)
            {
                var topic = string.Join(" ", trail);
                File.WriteAllText(Path.Combine(dir, $"note-{i}.md"),
                    $"# {Title(topic)} {i}\n\nEntry {i} about {topic}. Checked the {topic} again and wrote down what changed.\n");
            }
        }
        Directory.CreateDirectory(root);
        foreach (var a in Areas)
        {
            Make(Path.Combine(root, a), 0, [a]);
        }
        return root;
    }

    private static string Title(string s) => s.Length == 0 ? s : char.ToUpperInvariant(s[0]) + s[1..];
}
