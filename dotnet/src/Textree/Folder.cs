using System.Runtime.InteropServices;
using System.Text.Json.Nodes;

namespace Textree;

/// <summary>Why the engine did not do what was asked.</summary>
/// <param name="Kind"><c>usage</c> (the request is wrong), <c>refused</c> (it would break an
/// invariant, or a plan is not what its operations give), <c>stale</c> (the folder changed since
/// the plan was made) or <c>failed</c> (writing failed and was undone).</param>
/// <param name="Message">The lines the command line prints for the same input.</param>
public sealed record EngineError(string Kind, string Message);

/// <summary>The outcome of one call: a result or an error.</summary>
public sealed record EngineResult(JsonNode? Value, EngineError? Error)
{
    public bool Ok => Error is null;
}

/// <summary>
/// A folder of Markdown documents, seen through the Textree engine. Every method reads the folder
/// as it is at the time of the call. Nothing is written except by <see cref="Apply"/>.
/// </summary>
public sealed class Folder
{
    public Folder(string root) => Root = root;

    /// <summary>The folder.</summary>
    public string Root { get; }

    /// <summary>The relation graph: documents, links, tags, backlinks, broken links, orphans.</summary>
    public EngineResult Graph() => Call("graph", null);

    /// <summary>The links that do not resolve.</summary>
    public EngineResult Check() => Call("check", null);

    /// <summary>The folder and document hierarchy, from <paramref name="path"/> (default: the root).</summary>
    public EngineResult Tree(string? path = null) => Call("tree", new JsonObject { ["path"] = path });

    /// <summary>A document's text, headings, tags, links and the links that come to it.</summary>
    public EngineResult ReadDocument(string path) => Call("read_document", new JsonObject { ["path"] = path });

    /// <summary>Plans an operation (<c>{"op": "move", ...}</c>). Writes nothing.</summary>
    public EngineResult Plan(JsonNode operation, JsonNode? policy = null) =>
        Call("plan", new JsonObject { ["operation"] = operation.DeepClone(), ["policy"] = policy?.DeepClone() });

    /// <summary>
    /// Writes a plan, only if nothing it read has changed and it is exactly what its operations
    /// give on the folder now.
    /// </summary>
    public EngineResult Apply(JsonNode plan) => Call("apply", new JsonObject { ["plan"] = plan.DeepClone() });

    private EngineResult Call(string op, JsonObject? args)
    {
        var request = new JsonObject { ["op"] = op, ["root"] = Root, ["args"] = args ?? new JsonObject() };
        var response = JsonNode.Parse(Native.Call(request.ToJsonString()))!;
        if (response["ok"]?.GetValue<bool>() == true)
        {
            return new EngineResult(response["result"]?.DeepClone(), null);
        }
        var error = response["error"]!;
        return new EngineResult(null, new EngineError(
            error["kind"]?.GetValue<string>() ?? "failed",
            error["message"]?.GetValue<string>() ?? ""));
    }
}

internal static partial class Native
{
    private const string Library = "textree_ffi";

    [LibraryImport(Library, EntryPoint = "textree_call")]
    private static partial IntPtr TextreeCall(IntPtr request);

    [LibraryImport(Library, EntryPoint = "textree_free")]
    private static partial void TextreeFree(IntPtr response);

    public static string Call(string request)
    {
        var input = Marshal.StringToCoTaskMemUTF8(request);
        try
        {
            var output = TextreeCall(input);
            if (output == IntPtr.Zero)
            {
                return """{"ok":false,"error":{"kind":"failed","message":"error: no response\n"}}""";
            }
            try
            {
                return Marshal.PtrToStringUTF8(output) ?? "";
            }
            finally
            {
                TextreeFree(output);
            }
        }
        finally
        {
            Marshal.FreeCoTaskMem(input);
        }
    }
}
