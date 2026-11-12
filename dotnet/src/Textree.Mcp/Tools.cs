using System.ComponentModel;
using System.Text.Json;
using System.Text.Json.Nodes;
using ModelContextProtocol.Protocol;
using ModelContextProtocol.Server;

namespace Textree.Mcp;

/// <summary>
/// The tools. Each carries an engine result as it is: the structured content is the engine's
/// JSON, and the text content is the same JSON. Refusals and usage errors are tool results with
/// <c>isError</c> and the text the command line prints for the same input.
/// </summary>
[McpServerToolType]
public sealed class Tools(Folder folder)
{
    public const string Instructions =
        "A folder of Markdown documents (the knowledge folder). Read it with tree, read_document, graph and check. " +
        "To change it, call move, delete, split, merge, add_document or replace_document: each returns a plan and writes nothing. " +
        "Then pass that plan, unchanged, to apply_plan, which is the only tool that writes. " +
        "Links between documents are kept working: a plan rewrites the links a change would break, and a change that cannot keep them is refused with the reason. " +
        "A plan is refused if it was edited, or if the folder changed since it was made — make it again. " +
        "The engine does not judge whether any text is sensitive.";

    private static readonly JsonSerializerOptions Compact = new() { WriteIndented = false };

    // ---- reading ------------------------------------------------------------------------------

    [McpServerTool(Name = "tree", Title = "Folder hierarchy", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("The hierarchy of folders and documents (with each document's first top-level heading as its title). Give a folder path to see only that part.")]
    public CallToolResult Tree(
        [Description("A folder inside the knowledge folder, relative to it, '/'-separated. Leave out for the whole folder.")] string? path = null)
        => Result(folder.Tree(path));

    [McpServerTool(Name = "read_document", Title = "Read a document", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("A document's full text, its headings and tags, the links in it (with what each points at), and the links from other documents that point at it.")]
    public CallToolResult ReadDocument(
        [Description("The document, relative to the knowledge folder, e.g. 'notes/trip.md'.")] string path)
        => Result(folder.ReadDocument(path));

    [McpServerTool(Name = "graph", Title = "Relation graph", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("The relation graph of the whole folder: every document with its headings and tags, every link and what it resolves to, backlinks, tags, orphans (documents nothing links to) and broken links.")]
    public CallToolResult Graph() => Result(folder.Graph());

    [McpServerTool(Name = "check", Title = "Broken links", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("The links that do not resolve, each as document and line.")]
    public CallToolResult Check() => Result(folder.Check());

    // ---- planning (writes nothing) ----------------------------------------------------------------

    [McpServerTool(Name = "move", Title = "Plan a move or rename", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("Plans moving or renaming a document, an asset or a folder. Links to it and from it are rewritten so they keep pointing at the same things. Returns the plan; nothing is written until apply_plan.")]
    public CallToolResult Move(
        [Description("What to move, relative to the knowledge folder.")] string from,
        [Description("Its new full path, relative to the knowledge folder.")] string to,
        [Description("Refuse a change that would touch more than this many nodes.")] long? max_touched = null,
        [Description("Refuse a change that would rewrite more than this many bytes of links.")] long? max_bytes = null)
        => Plan(new JsonObject { ["op"] = "move", ["from"] = from, ["to"] = to }, false, max_touched, max_bytes);

    [McpServerTool(Name = "delete", Title = "Plan a delete", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("Plans deleting a document, an asset or a folder with everything in it. Refused while other documents link to it, unless break_links is true. Returns the plan; nothing is written until apply_plan.")]
    public CallToolResult Delete(
        [Description("What to delete, relative to the knowledge folder.")] string path,
        [Description("Allow links to the deleted things to be left pointing at nothing.")] bool break_links = false,
        [Description("Refuse a change that would touch more than this many nodes.")] long? max_touched = null,
        [Description("Refuse a change that would rewrite more than this many bytes of links.")] long? max_bytes = null)
        => Plan(new JsonObject { ["op"] = "delete", ["path"] = path }, break_links, max_touched, max_bytes);

    [McpServerTool(Name = "split", Title = "Plan a split", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("Plans splitting a document at its headings of one level: it becomes a folder whose own document keeps the text before the first such heading and a list of links, and each section becomes a document. Links that named a section follow it. Returns the plan; nothing is written until apply_plan.")]
    public CallToolResult Split(
        [Description("The document to split, relative to the knowledge folder.")] string path,
        [Description("The heading level to split at (1-6, default 2).")] long level = 2,
        [Description("Refuse a change that would touch more than this many nodes.")] long? max_touched = null,
        [Description("Refuse a change that would rewrite more than this many bytes of links.")] long? max_bytes = null)
        => Plan(new JsonObject { ["op"] = "split", ["path"] = path, ["level"] = level }, false, max_touched, max_bytes);

    [McpServerTool(Name = "merge", Title = "Plan a merge", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("Plans merging documents into another one: each becomes a section of it (its headings one level lower) and is removed. Links to them follow to their sections. Returns the plan; nothing is written until apply_plan.")]
    public CallToolResult Merge(
        [Description("The document that receives the others.")] string into,
        [Description("The documents to merge in, in the order of their sections.")] string[] from,
        [Description("Refuse a change that would touch more than this many nodes.")] long? max_touched = null,
        [Description("Refuse a change that would rewrite more than this many bytes of links.")] long? max_bytes = null)
        => Plan(new JsonObject { ["op"] = "merge", ["into"] = into, ["from"] = new JsonArray(from.Select(f => (JsonNode?)JsonValue.Create(f)).ToArray()) }, false, max_touched, max_bytes);

    [McpServerTool(Name = "add_document", Title = "Plan a new document", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("Plans adding a new document with this Markdown text (never over an existing one). If its name would take over another document's name links, those links are rewritten to keep their meaning. Links in the new text that do not resolve are allowed and listed in new_broken. Returns the plan; nothing is written until apply_plan.")]
    public CallToolResult AddDocument(
        [Description("The new document, relative to the knowledge folder, ending in .md.")] string path,
        [Description("Its Markdown text.")] string content,
        [Description("Refuse a change that would touch more than this many nodes.")] long? max_touched = null,
        [Description("Refuse a change that would rewrite more than this many bytes of links.")] long? max_bytes = null)
        => Plan(new JsonObject { ["op"] = "add", ["path"] = path, ["content"] = content }, false, max_touched, max_bytes);

    [McpServerTool(Name = "replace_document", Title = "Plan new text for a document", ReadOnly = true, Destructive = false, Idempotent = true, OpenWorld = false)]
    [Description("Plans replacing the whole text of a document. Links from other documents keep working; a new text that drops a heading another document links to is refused. Documents left with no links to them are listed in orphaned. Returns the plan; nothing is written until apply_plan.")]
    public CallToolResult ReplaceDocument(
        [Description("The document, relative to the knowledge folder.")] string path,
        [Description("Its new Markdown text.")] string content,
        [Description("Refuse a change that would touch more than this many nodes.")] long? max_touched = null,
        [Description("Refuse a change that would rewrite more than this many bytes of links.")] long? max_bytes = null)
        => Plan(new JsonObject { ["op"] = "replace", ["path"] = path, ["content"] = content }, false, max_touched, max_bytes);

    // ---- writing --------------------------------------------------------------------------------

    [McpServerTool(Name = "apply_plan", Title = "Apply a plan", ReadOnly = false, Destructive = true, Idempotent = true, OpenWorld = false)]
    [Description("Writes a plan returned by move, delete, split, merge, add_document or replace_document. Pass the plan unchanged. It is written only if nothing it read has changed and it is exactly what its operations give on the folder now; otherwise it is refused and nothing is written. Applying the same plan twice changes nothing the second time.")]
    public CallToolResult ApplyPlan(
        [Description("The plan object, exactly as a planning tool returned it.")] JsonElement plan)
        => Result(folder.Apply(JsonNode.Parse(plan.GetRawText())!));

    // ---- helpers --------------------------------------------------------------------------------

    private CallToolResult Plan(JsonObject operation, bool breakLinks, long? maxTouched, long? maxBytes)
    {
        var policy = new JsonObject
        {
            ["break_links"] = breakLinks,
            ["max_touched"] = maxTouched,
            ["max_bytes"] = maxBytes,
        };
        return Result(folder.Plan(operation, policy));
    }

    private static CallToolResult Result(EngineResult result)
    {
        if (result.Error is { } error)
        {
            return new CallToolResult
            {
                IsError = true,
                Content = [new TextContentBlock { Text = error.Message }],
            };
        }
        var value = result.Value ?? new JsonObject();
        var json = value.ToJsonString(Compact);
        return new CallToolResult
        {
            Content = [new TextContentBlock { Text = json }],
            StructuredContent = JsonElement.Parse(json),
        };
    }
}
