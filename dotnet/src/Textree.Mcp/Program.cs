using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using ModelContextProtocol.Protocol;
using ModelContextProtocol.Server;
using Textree;
using Textree.Mcp;

// textree-mcp --root <folder>: an MCP server over stdio. Standard output carries MCP messages only.
string? root = null;
for (var i = 0; i < args.Length; i++)
{
    if (args[i] == "--root" && i + 1 < args.Length)
    {
        root = args[++i];
    }
    else
    {
        Console.Error.WriteLine($"error: unexpected argument: {args[i]}");
        return 2;
    }
}
if (root is null)
{
    Console.Error.WriteLine("usage: Textree.Mcp --root <folder>");
    return 2;
}
if (!Directory.Exists(root))
{
    Console.Error.WriteLine($"error: {root}: not a folder");
    return 2;
}

// An empty builder: no configuration from the environment or files, no logging to the console.
var builder = Host.CreateEmptyApplicationBuilder(settings: null);
builder.Services.AddSingleton(new Folder(Path.GetFullPath(root)));
builder.Services
    .AddMcpServer(options =>
    {
        options.ServerInfo = new() { Name = "textree", Version = typeof(Folder).Assembly.GetName().Version?.ToString(3) ?? "0.0.0" };
        options.ServerInstructions = Tools.Instructions;
    })
    .WithStdioServerTransport()
    .WithTools<Tools>()
    .WithRequestFilters(filters => filters.AddCallToolFilter(next => async (context, cancellationToken) =>
    {
        if (context.MatchedPrimitive is McpServerTool tool
            && Arguments.Problem(tool.ProtocolTool, context.Params?.Arguments) is { } problem)
        {
            return new CallToolResult { IsError = true, Content = [new TextContentBlock { Text = problem + "\n" }] };
        }
        return await next(context, cancellationToken);
    }));
await builder.Build().RunAsync();
return 0;
