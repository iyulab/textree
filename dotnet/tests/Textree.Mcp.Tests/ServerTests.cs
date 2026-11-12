using System.Text.Json;
using System.Text.Json.Nodes;
using ModelContextProtocol.Client;
using ModelContextProtocol.Protocol;
using Xunit;

namespace Textree.Mcp.Tests;

/// <summary>A folder in the temporary directory, removed afterwards.</summary>
public sealed class Scratch : IDisposable
{
    public Scratch()
    {
        Root = Path.Combine(Path.GetTempPath(), "textree-mcp-test-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Root);
    }

    public string Root { get; }

    public Scratch File(string path, string text)
    {
        var full = Path.Combine(Root, path);
        Directory.CreateDirectory(Path.GetDirectoryName(full)!);
        System.IO.File.WriteAllText(full, text);
        return this;
    }

    public string Read(string path) => System.IO.File.ReadAllText(Path.Combine(Root, path));

    public bool Exists(string path) => System.IO.File.Exists(Path.Combine(Root, path));

    /// <summary>Names and bytes of every file.</summary>
    public string State() => string.Join("\n", Directory
        .EnumerateFiles(Root, "*", SearchOption.AllDirectories)
        .Order(StringComparer.Ordinal)
        .Select(f => Path.GetRelativePath(Root, f) + "=" + Convert.ToBase64String(System.IO.File.ReadAllBytes(f))));

    public void Dispose()
    {
        try { Directory.Delete(Root, recursive: true); } catch (IOException) { }
    }
}

public sealed class ServerTests
{
    private static async Task<McpClient> Start(Scratch s)
    {
        var server = Path.Combine(AppContext.BaseDirectory, "Textree.Mcp.dll");
        var transport = new StdioClientTransport(new StdioClientTransportOptions
        {
            Name = "textree",
            Command = "dotnet",
            Arguments = [server, "--root", s.Root],
        });
        return await McpClient.CreateAsync(transport, cancellationToken: TestContext.Current.CancellationToken);
    }

    private static Scratch Folder() => new Scratch()
        .File("index.md", "# Index\n\nSee [trip](notes/trip.md) and [[plan]].\n")
        .File("notes/trip.md", "# Trip\n\nBack to [[index]].\n")
        .File("plan.md", "# Plan\n\nThe [[trip]].\n");

    private static Task<CallToolResult> Call(McpClient client, string tool, object? args = null)
    {
        var arguments = args is null
            ? new Dictionary<string, object?>()
            : JsonSerializer.SerializeToElement(args).EnumerateObject().ToDictionary(p => p.Name, p => (object?)p.Value);
        return client.CallToolAsync(tool, arguments, cancellationToken: TestContext.Current.CancellationToken).AsTask();
    }

    private static string Text(CallToolResult r) => ((TextContentBlock)r.Content[0]).Text;

    [Fact]
    public async Task Lists_eleven_tools_with_safety_hints()
    {
        using var s = Folder();
        await using var client = await Start(s);
        var tools = await client.ListToolsAsync(cancellationToken: TestContext.Current.CancellationToken);
        var names = tools.Select(t => t.Name).Order(StringComparer.Ordinal).ToArray();
        Assert.Equal(
            new[] { "add_document", "apply_plan", "check", "delete", "graph", "merge", "move", "read_document", "replace_document", "split", "tree" },
            names);
        foreach (var tool in tools)
        {
            Assert.Matches("^[a-zA-Z0-9_-]+$", tool.Name);
            var a = tool.ProtocolTool.Annotations!;
            var writes = tool.Name == "apply_plan";
            Assert.Equal(!writes, a.ReadOnlyHint);
            Assert.Equal(writes, a.DestructiveHint);
            Assert.True(a.IdempotentHint);
            Assert.False(a.OpenWorldHint);
        }
    }

    [Fact]
    public async Task A_plan_writes_nothing_and_apply_plan_writes_it_once()
    {
        using var s = Folder();
        await using var client = await Start(s);
        var before = s.State();
        var plan = await Call(client, "move", new { from = "notes/trip.md", to = "archive/trip.md" });
        Assert.NotEqual(true, plan.IsError);
        Assert.Equal(before, s.State());
        var planObject = JsonNode.Parse(Text(plan))!;
        Assert.Equal("move", planObject["ops"]![0]!["op"]!.GetValue<string>());
        Assert.Equal(plan.StructuredContent!.Value.GetRawText(), Text(plan));

        var applied = await Call(client, "apply_plan", new { plan = planObject });
        Assert.NotEqual(true, applied.IsError);
        Assert.True(s.Exists("archive/trip.md"));
        Assert.Equal("# Index\n\nSee [trip](archive/trip.md) and [[plan]].\n", s.Read("index.md"));

        var after = s.State();
        var again = await Call(client, "apply_plan", new { plan = planObject });
        Assert.True(again.IsError);
        Assert.StartsWith("stale: ", Text(again));
        Assert.Equal(after, s.State());
    }

    [Fact]
    public async Task A_tampered_plan_is_refused()
    {
        using var s = Folder();
        await using var client = await Start(s);
        var plan = JsonNode.Parse(Text(await Call(client, "move", new { from = "notes/trip.md", to = "archive/trip.md" })))!;
        plan["writes"]![0]!["content"] = "PWNED\n";
        var before = s.State();
        var r = await Call(client, "apply_plan", new { plan });
        Assert.True(r.IsError);
        Assert.Equal("refused [plan]: the plan does not match what its operations produce on this folder\n", Text(r));
        Assert.Equal(before, s.State());
    }

    [Fact]
    public async Task Refusals_and_bad_arguments_are_tool_errors_and_the_server_keeps_going()
    {
        using var s = Folder();
        await using var client = await Start(s);
        var refused = await Call(client, "delete", new { path = "plan.md" });
        Assert.True(refused.IsError);
        Assert.Equal("refused [I2] index.md:3: [[plan]] would break\n", Text(refused));
        var missing = await Call(client, "read_document");
        Assert.True(missing.IsError);
        Assert.Equal("error: missing argument: path\n", Text(missing));
        var outside = await Call(client, "read_document", new { path = "../x.md" });
        Assert.True(outside.IsError);
        Assert.StartsWith("error: ", Text(outside));
        var check = await Call(client, "check");
        Assert.NotEqual(true, check.IsError);
        Assert.Equal("no broken links\n", JsonNode.Parse(Text(check))!["text"]!.GetValue<string>());
    }

    [Fact]
    public async Task Adds_a_document_through_a_plan()
    {
        using var s = Folder();
        await using var client = await Start(s);
        var plan = await Call(client, "add_document", new { path = "notes/new.md", content = "# New\n\nSee [[trip]]." });
        Assert.NotEqual(true, plan.IsError);
        Assert.False(s.Exists("notes/new.md"));
        var applied = await Call(client, "apply_plan", new { plan = JsonNode.Parse(Text(plan)) });
        Assert.NotEqual(true, applied.IsError);
        Assert.Equal("# New\n\nSee [[trip]].\n", s.Read("notes/new.md"));
    }
}
