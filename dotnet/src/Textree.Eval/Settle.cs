using System.Text;
using System.Text.Json.Nodes;
using Microsoft.Extensions.AI;
using Textree.Ingest;

namespace Textree.Eval;

/// <summary>
/// The runner as a consumer that keeps settled records. Every planned case is settled, the three
/// decisions taken in turn so each kind of line is written: accept the place, correct it to another
/// folder (an accepted one when the harness missed, else the parent folder), or reject the proposal.
/// The records are read back by the decision library that reads that line format.
/// </summary>
public static class Settle
{
    public static async Task<JsonObject> RunAsync(string corpus, IChatClient chat, string recordsDir, CancellationToken ct = default)
    {
        if (Directory.Exists(recordsDir))
        {
            Directory.Delete(recordsDir, recursive: true);
        }
        var log = new JsonLinesSettlements(recordsDir);
        var at = new DateTimeOffset(2026, 11, 13, 9, 0, 0, TimeSpan.Zero);
        var rows = new JsonArray();
        var strayFiles = 0;
        var turn = 0;
        foreach (var file in Directory.EnumerateFiles(Path.Combine(corpus, "expected"), "*.json").Order(StringComparer.Ordinal))
        {
            var expected = JsonNode.Parse(File.ReadAllText(file))!.AsObject();
            if (expected["expect"]?.GetValue<string>() == "empty")
            {
                continue;
            }
            var tree = Path.Combine(corpus, expected["tree"]?.GetValue<string>() ?? "tree");
            var input = File.ReadAllText(Path.Combine(corpus, expected["ingest"]!.GetValue<string>()));
            using var ws = new Workspace(tree);
            var before = Workspace.Files(ws.Root);
            var result = await new Ingester(new Folder(ws.Root), chat, log: log).IngestAsync(new IngestRequest(input, Source: Path.GetFileName(file)), null, ct);
            // Ingesting writes nothing into the knowledge folder; only the consumer's records grow.
            strayFiles += Gates.Changed(before, Workspace.Files(ws.Root)).Count;
            if (result.Outcome != "planned")
            {
                rows.Add(new JsonObject { ["case"] = Path.GetFileNameWithoutExtension(file), ["outcome"] = result.Outcome });
                continue;
            }
            var id = Ingester.ProposalId(result.Plan!);
            var accepted = Accepted(expected).ToList();
            var landing = JsonLinesSettlements.FolderOf(result.Placement[0]);
            var name = Path.GetFileNameWithoutExtension(file);
            at = at.AddMinutes(5);
            switch (turn++ % 3)
            {
                case 0:
                    log.Settle(id, input, SettlementOutcome.Accepted, at);
                    rows.Add(new JsonObject { ["case"] = name, ["settled"] = "accepted", ["folder"] = landing });
                    break;
                case 1:
                    var other = accepted.FirstOrDefault(a => a != landing) ?? Hierarchy.DirOf(landing);
                    log.Settle(id, input, SettlementOutcome.Corrected, at, other);
                    rows.Add(new JsonObject { ["case"] = name, ["settled"] = "corrected", ["from"] = landing, ["to"] = other });
                    break;
                default:
                    log.Settle(id, input, SettlementOutcome.Rejected, at);
                    rows.Add(new JsonObject { ["case"] = name, ["settled"] = "rejected", ["folder"] = landing });
                    break;
            }
        }
        var lines = File.Exists(log.SettledPath) ? File.ReadAllText(log.SettledPath) : "";
        string? readError = null;
        var read = 0;
        try
        {
            using var records = Gil.SettledRecords.FromJsonLines(lines);
            read = records.Count;
        }
        catch (Exception e) when (e is ArgumentException or FormatException or InvalidOperationException)
        {
            readError = e.Message;
        }
        return new JsonObject
        {
            ["kind"] = "settle",
            ["settled_lines"] = lines.Split('\n', StringSplitOptions.RemoveEmptyEntries).Length,
            ["read_by_gil"] = read,
            ["read_error"] = readError,
            ["decisions"] = Json.Object(new SortedDictionary<string, int>(rows.Where(r => r!["settled"] is not null)
                .GroupBy(r => r!["settled"]!.GetValue<string>()).ToDictionary(g => g.Key, g => g.Count()), StringComparer.Ordinal)),
            ["knowledge_folder_files_changed"] = strayFiles,
            ["rows"] = rows,
        };
    }

    /// <summary>The folders a case accepts, from its expected places.</summary>
    private static IEnumerable<string> Accepted(JsonObject expected)
    {
        foreach (var a in expected["placement"]?["any_of"]?.AsArray() ?? [])
        {
            if (a!["edit"] is { } e)
            {
                yield return Hierarchy.DirOf(e.GetValue<string>());
            }
            else if (a["add_under"] is { } u)
            {
                yield return u.GetValue<string>().TrimEnd('/');
            }
            else if (a["in"] is { } f)
            {
                yield return f.GetValue<string>().TrimEnd('/');
            }
        }
    }
}
