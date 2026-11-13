using System.Text.Json;
using System.Text.Json.Nodes;

namespace Textree.Ingest;

/// <summary>How a person settled a proposal.</summary>
public enum SettlementOutcome
{
    /// <summary>The proposed place was taken.</summary>
    Accepted,
    /// <summary>The text went to another place the person chose.</summary>
    Corrected,
    /// <summary>The proposal was turned down and the text was not filed.</summary>
    Rejected,
}

/// <summary>
/// A consumer-side record of proposals and how people settled them, as JSON lines in a folder the
/// consumer chooses (never the knowledge folder): <c>proposals.jsonl</c> (what was offered) and
/// <c>settled.jsonl</c> (one line per settlement, in the settled-records line format — <c>id</c>,
/// <c>settled_at</c>, <c>values</c>, <c>settlements</c>, <c>suggested</c>, <c>sources</c> — so a
/// decision library that reads that format can learn from them). The value settled is the folder the
/// text went to. Nothing here is needed to ingest; it is for consumers that keep the records.
/// </summary>
public sealed class JsonLinesSettlements : IProposalLog
{
    private readonly string _proposals;
    private readonly string _settled;
    private readonly Dictionary<string, Proposal> _open = new(StringComparer.Ordinal);
    private static readonly JsonSerializerOptions Compact = new() { WriteIndented = false, Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

    public JsonLinesSettlements(string directory)
    {
        Directory.CreateDirectory(directory);
        _proposals = Path.Combine(directory, "proposals.jsonl");
        _settled = Path.Combine(directory, "settled.jsonl");
    }

    public string SettledPath => _settled;

    public void Proposed(Proposal proposal)
    {
        _open[proposal.Id] = proposal;
        var line = new JsonObject
        {
            ["id"] = proposal.Id,
            ["at"] = proposal.At.ToString("O"),
            ["source"] = proposal.Source,
            ["placement"] = JsonSerializer.SerializeToNode(proposal.Placement, Compact),
        };
        File.AppendAllText(_proposals, line.ToJsonString(Compact) + "\n");
    }

    /// <summary>
    /// Records how a proposal was settled. <paramref name="text"/> is the text that was ingested
    /// (the observed value); <paramref name="folder"/> is where it went — for a correction the folder
    /// the person chose; ignored for an acceptance (the proposal's own) and a rejection.
    /// </summary>
    public void Settle(string proposalId, string text, SettlementOutcome outcome, DateTimeOffset at, string? folder = null)
    {
        if (!_open.TryGetValue(proposalId, out var proposal))
        {
            throw new ArgumentException($"no open proposal {proposalId}", nameof(proposalId));
        }
        var suggested = FolderOf(proposal.Placement[0]);
        var values = new JsonObject { ["text"] = text };
        var settledFolder = outcome switch
        {
            SettlementOutcome.Accepted => suggested,
            SettlementOutcome.Corrected => folder ?? throw new ArgumentException("a correction names the folder", nameof(folder)),
            _ => null,
        };
        if (settledFolder is not null)
        {
            values["folder"] = Show(settledFolder);
        }
        var line = new JsonObject
        {
            ["id"] = proposalId,
            ["settled_at"] = (at.UtcTicks - DateTimeOffset.UnixEpoch.UtcTicks),
            ["values"] = values,
            ["settlements"] = new JsonObject
            {
                // The harness is not the reader's own decider: what people did with its proposal is "other".
                ["folder"] = outcome switch
                {
                    SettlementOutcome.Accepted => "AcceptOther",
                    SettlementOutcome.Corrected => "CorrectOther",
                    _ => "Reject",
                },
            },
            ["suggested"] = new JsonObject { ["folder"] = Show(suggested) },
        };
        File.AppendAllText(_settled, line.ToJsonString(Compact) + "\n");
        _open.Remove(proposalId);
    }

    /// <summary>The folder a placement puts its text in ("(top)" for the top folder).</summary>
    public static string FolderOf(Placement p) =>
        p.Form == "add_folder" ? Hierarchy.DirOf(Hierarchy.DirOf(p.Target)) : Hierarchy.DirOf(p.Target);

    private static string Show(string folder) => folder.Length == 0 ? "(top)" : folder.Normalize(System.Text.NormalizationForm.FormC);
}
