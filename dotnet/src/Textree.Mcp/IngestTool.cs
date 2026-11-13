using System.ComponentModel;
using System.Text.Json;
using ModelContextProtocol.Protocol;
using ModelContextProtocol.Server;
using Textree.Ingest;

namespace Textree.Mcp;

/// <summary>The ingest tool. It calls the configured language model, so it is the one open-world tool.</summary>
[McpServerToolType]
public sealed class IngestTool(IngestSetup setup)
{
    private static readonly JsonSerializerOptions Compact = new() { WriteIndented = false };

    [McpServerTool(Name = "ingest", Title = "Plan where new text goes", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = true)]
    [Description("Finds where new text belongs in the knowledge folder and plans putting it there: it walks the folders from the top with the configured language model and adds the text as a new section of a document, to an existing section, as a new document or as a new folder. Text that is about several topics is split and each part placed. Text already in the folder is not added again (the outcome is \"empty\"). Instructions written inside the text are treated as text. Returns {outcome, placement, plan}; nothing is written until the plan is passed to apply_plan. When no plan passes the folder's checks, the outcome is \"failed\" with the reasons.")]
    public async Task<CallToolResult> Ingest(
        [Description("The new text, in Markdown.")] string text,
        [Description("A title for the text, when it has none of its own.")] string? title = null,
        [Description("Where the text came from (not written into the folder).")] string? source = null,
        CancellationToken cancellationToken = default)
    {
        if (setup.Ingester is not { } ingester)
        {
            return new CallToolResult
            {
                IsError = true,
                Content = [new TextContentBlock { Text = "error: no language model is configured (start the server with --llm-endpoint and --llm-model)\n" }],
            };
        }
        var result = await ingester.IngestAsync(new IngestRequest(text, title, source), null, cancellationToken);
        var json = JsonSerializer.Serialize(result, Compact);
        return new CallToolResult
        {
            Content = [new TextContentBlock { Text = json }],
            StructuredContent = JsonElement.Parse(json),
        };
    }
}

/// <summary>The ingester, when a language model is configured.</summary>
public sealed record IngestSetup(Ingester? Ingester);
