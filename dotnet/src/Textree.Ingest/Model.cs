using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace Textree.Ingest;

/// <summary>New text to place.</summary>
/// <param name="Text">The Markdown text.</param>
/// <param name="Title">A title for it, if it has one that is not in the text.</param>
/// <param name="Source">Where it came from (kept out of the text; for the consumer's records).</param>
public sealed record IngestRequest(string Text, string? Title = null, string? Source = null);

/// <summary>How to ingest.</summary>
/// <param name="MaxRepairs">How many times a refused plan is made again before giving up.</param>
/// <param name="MaxDepth">How many folders deep the walk goes at most.</param>
public sealed record IngestOptions(int MaxRepairs = 3, int MaxDepth = 8);

/// <summary>Where one piece of the text goes.</summary>
/// <param name="Piece">The piece, numbered from 1 in the order of the text.</param>
/// <param name="Form"><c>append_section</c>, <c>add_to_section</c>, <c>add_document</c> or <c>add_folder</c>.</param>
/// <param name="Target">The document written (for a new folder: its own document).</param>
/// <param name="Heading">The section heading it goes under, when there is one.</param>
public sealed record Placement(
    [property: JsonPropertyName("piece")] int Piece,
    [property: JsonPropertyName("form")] string Form,
    [property: JsonPropertyName("target")] string Target,
    [property: JsonPropertyName("heading"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Heading);

/// <summary>The outcome of an ingest.</summary>
public sealed class IngestResult
{
    /// <summary><c>planned</c> (a checked plan), <c>empty</c> (everything is already there) or <c>failed</c>.</summary>
    [JsonPropertyName("outcome")] public required string Outcome { get; init; }

    [JsonPropertyName("placement")] public IReadOnlyList<Placement> Placement { get; init; } = [];

    /// <summary>The plan, for apply_plan — present when the outcome is <c>planned</c>.</summary>
    [JsonPropertyName("plan"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public JsonNode? Plan { get; init; }

    /// <summary>Why it failed: the last refusal's lines, each led by the operation that ran into it
    /// (<c>add_document: refused [I5] con.md: …</c>), or what went wrong.</summary>
    [JsonPropertyName("violations"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<string>? Violations { get; init; }

    /// <summary>How it was decided — kept for evaluation, not part of a tool's answer.</summary>
    [JsonIgnore] public IngestTrace Trace { get; init; } = new();
}

/// <summary>What the harness did: each model call and each step of the walk.</summary>
public sealed class IngestTrace
{
    public List<TraceStep> Steps { get; } = [];
    public int ModelCalls { get; set; }
    public long InputTokens { get; set; }
    public long OutputTokens { get; set; }
    public int Repairs { get; set; }
    /// <summary>Model answers refused because they named something they were not shown.</summary>
    public int UnseenCitations { get; set; }
    public TimeSpan Elapsed { get; set; }
}

/// <summary>One decision: where the walk was, what was shown, what the model answered.</summary>
public sealed record TraceStep(int Piece, string At, IReadOnlyList<string> Shown, string Answer, string? Refusal);
