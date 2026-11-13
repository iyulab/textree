using System.Text.Json;
using Textree.Ingest;

namespace Textree.Eval;

public static class Cli
{
    private static readonly JsonSerializerOptions Pretty = new() { WriteIndented = true, Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

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
            default:
                Console.Error.WriteLine($"unknown command: {args[0]}");
                return 2;
        }
    }
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
