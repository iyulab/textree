using System.Text.Json;
using Textree.Ingest;

namespace Textree.Eval;

public static class Cli
{
    private static readonly JsonSerializerOptions Pretty = new() { WriteIndented = true, Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

    /// <summary>The model: live, live and recorded (--record), or replayed from a recording (--replay).</summary>
    private static (Microsoft.Extensions.AI.IChatClient Chat, RecordingChatClient? Recorder) Chat(Options opts)
    {
        if (opts.Get("replay") is { } replay)
        {
            return (RecordingChatClient.Replay(replay), null);
        }
        var live = Llm.Connect(opts.Require("llm-endpoint"), opts.Require("llm-model"), opts.Get("key-env"));
        if (opts.Get("record") is { } record)
        {
            var recorder = RecordingChatClient.Record(live, record);
            return (recorder, recorder);
        }
        return (live, null);
    }

    /// <summary>Writes a report: to --out, else to standard output. A replayed report has no time stamp.</summary>
    private static void Write(Options opts, System.Text.Json.Nodes.JsonObject report)
    {
        if (!opts.Has("replay"))
        {
            report["machine"] = new System.Text.Json.Nodes.JsonObject { ["generated_at"] = DateTimeOffset.UtcNow.ToString("O") };
        }
        var text = report.ToJsonString(Pretty) + "\n";
        if (opts.Get("out") is { } path)
        {
            File.WriteAllText(path, text);
        }
        else
        {
            Console.Write(text);
        }
    }

    public static async Task<int> RunAsync(string[] args)
    {
        if (args.Length == 0)
        {
            Console.Error.WriteLine("usage: Textree.Eval try --root <folder> --text <file> [--apply] --llm-endpoint <url> --llm-model <name>");
            return 2;
        }
        var opts = Options.Parse(args.Skip(1));
        switch (args[0])
        {
            case "try":
            {
                var folder = new Folder(Path.GetFullPath(opts.Require("root")));
                var chat = Llm.Connect(opts.Require("llm-endpoint"), opts.Require("llm-model"));
                var ingester = new Ingester(folder, chat);
                var result = await ingester.IngestAsync(new IngestRequest(File.ReadAllText(opts.Require("text"))));
                Console.WriteLine(JsonSerializer.Serialize(new { result.Outcome, result.Placement, result.Violations, trace = new { result.Trace.ModelCalls, result.Trace.InputTokens, result.Trace.OutputTokens, result.Trace.Repairs, result.Trace.UnseenCitations, elapsed = result.Trace.Elapsed.TotalSeconds, steps = result.Trace.Steps } }, Pretty));
                if (opts.Has("apply") && result.Plan is not null)
                {
                    var applied = folder.Apply(result.Plan);
                    Console.WriteLine(applied.Ok ? applied.Value!.ToJsonString() : "refused: " + applied.Error!.Message);
                }
                return result.Outcome == "failed" ? 1 : 0;
            }
            case "cases":
            {
                var (chat, recorder) = Chat(opts);
                var report = await Cases.RunAsync(opts.Require("corpus"), chat, opts.Int("runs", 1));
                recorder?.Save();
                Write(opts, report);
                var failures = report["summary"]!["gate_failures"]!.AsObject().Count;
                return failures == 0 ? 0 : 1;
            }
            case "loo":
            {
                var (chat, recorder) = Chat(opts);
                var arms = (opts.Get("arms") ?? "harness,b0,b1,b2,gil").Split(',', StringSplitOptions.RemoveEmptyEntries).ToHashSet(StringComparer.Ordinal);
                var settings = new LeaveOneOut.Settings(opts.Int("per-stratum", 5), (ulong)opts.Int("seed", 1), opts.Int("runs", 1), opts.Int("min-chars", 200), opts.Has("unmasked"), arms);
                var report = await LeaveOneOut.RunAsync(opts.Require("root"), chat, settings);
                recorder?.Save();
                // A labeller of the panel: the single-call arm, run with another model, under its own name.
                if (opts.Get("label") is { } label)
                {
                    foreach (var part in new[] { "summary", "arms" })
                    {
                        if (report[part]?["b2"] is { } node)
                        {
                            report[part]!.AsObject().Remove("b2");
                            report[part]!.AsObject()[label] = node;
                        }
                    }
                    report["labeller"] = new System.Text.Json.Nodes.JsonObject { ["name"] = label, ["model"] = opts.Get("llm-model") };
                }
                Write(opts, report);
                return 0;
            }
            case "scale":
            {
                var (chat, recorder) = Chat(opts);
                var sizes = (opts.Get("sizes") ?? "100,1000,10000").Split(',').Select(int.Parse).ToList();
                var report = await Scale.RunAsync(chat, sizes);
                recorder?.Save();
                Write(opts, report);
                return 0;
            }
            case "settle":
            {
                var (chat, recorder) = Chat(opts);
                var report = await Settle.RunAsync(opts.Require("corpus"), chat, opts.Require("records"));
                recorder?.Save();
                Write(opts, report);
                return report["read_error"] is null ? 0 : 1;
            }
            case "rebuild":
            {
                var (chat, recorder) = Chat(opts);
                var folders = opts.Require("folders").Split(',', StringSplitOptions.RemoveEmptyEntries);
                var report = await Rebuild.RunAsync(opts.Require("root"), folders, chat, opts.Int("seed", 1));
                recorder?.Save();
                Write(opts, report);
                return 0;
            }
            default:
                Console.Error.WriteLine($"unknown command: {args[0]}");
                return 2;
        }
    }
}

public static partial class CliHelpers
{
}

/// <summary>--name value and --flag arguments.</summary>
public sealed class Options
{
    private readonly Dictionary<string, string?> _values = new();

    public static Options Parse(IEnumerable<string> args)
    {
        var o = new Options();
        var list = args.ToList();
        for (var i = 0; i < list.Count; i++)
        {
            if (!list[i].StartsWith("--", StringComparison.Ordinal))
            {
                throw new ArgumentException($"unexpected argument: {list[i]}");
            }
            var name = list[i][2..];
            var value = i + 1 < list.Count && !list[i + 1].StartsWith("--", StringComparison.Ordinal) ? list[++i] : null;
            o._values[name] = value;
        }
        return o;
    }

    public bool Has(string name) => _values.ContainsKey(name);
    public string? Get(string name) => _values.GetValueOrDefault(name);
    public string Require(string name) => Get(name) ?? throw new ArgumentException($"missing --{name}");
    public int Int(string name, int fallback) => int.TryParse(Get(name), out var v) ? v : fallback;
}
