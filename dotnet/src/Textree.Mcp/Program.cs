using System.ClientModel;
using Microsoft.Extensions.AI;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using ModelContextProtocol.Protocol;
using ModelContextProtocol.Server;
using OpenAI;
using Textree;
using Textree.Ingest;
using Textree.Mcp;

// Textree.Mcp --root <folder> [--llm-endpoint <url> --llm-model <name> [--llm-temperature <t>]]
// An MCP server over stdio. Standard output carries MCP messages only. The language model's key,
// if it needs one, is read from TEXTREE_LLM_KEY so it does not show in the process list.
string? root = null, endpoint = null, model = null;
float? temperature = null;
for (var i = 0; i < args.Length; i++)
{
    string? Next() => i + 1 < args.Length ? args[++i] : null;
    switch (args[i])
    {
        case "--root": root = Next(); break;
        case "--llm-endpoint": endpoint = Next(); break;
        case "--llm-model": model = Next(); break;
        case "--llm-temperature":
            if (!float.TryParse(Next(), System.Globalization.CultureInfo.InvariantCulture, out var t))
            {
                Console.Error.WriteLine("error: --llm-temperature takes a number");
                return 2;
            }
            temperature = t;
            break;
        default:
            Console.Error.WriteLine($"error: unexpected argument: {args[i]}");
            return 2;
    }
}
if (root is null)
{
    Console.Error.WriteLine("usage: Textree.Mcp --root <folder> [--llm-endpoint <url> --llm-model <name> [--llm-temperature <t>]]");
    return 2;
}
if (!Directory.Exists(root))
{
    Console.Error.WriteLine($"error: {root}: not a folder");
    return 2;
}
if ((endpoint is null) != (model is null))
{
    Console.Error.WriteLine("error: --llm-endpoint and --llm-model go together");
    return 2;
}

var folder = new Folder(Path.GetFullPath(root));
Ingester? ingester = null;
if (endpoint is not null && model is not null)
{
    var key = Environment.GetEnvironmentVariable("TEXTREE_LLM_KEY");
    var client = new OpenAIClient(new ApiKeyCredential(string.IsNullOrEmpty(key) ? "none" : key), new OpenAIClientOptions { Endpoint = new Uri(endpoint) });
    IChatClient chat = client.GetChatClient(model).AsIChatClient();
    ingester = temperature is null ? new Ingester(folder, chat) : new Ingester(folder, chat) { Sampling = new ChatOptions { Temperature = temperature } };
}

// An empty builder: no configuration from the environment or files, no logging to the console.
var builder = Host.CreateEmptyApplicationBuilder(settings: null);
builder.Services.AddSingleton(folder);
builder.Services.AddSingleton(new IngestSetup(ingester));
builder.Services
    .AddMcpServer(options =>
    {
        options.ServerInfo = new() { Name = "textree", Version = typeof(Folder).Assembly.GetName().Version?.ToString(3) ?? "0.0.0" };
        options.ServerInstructions = Tools.Instructions;
    })
    .WithStdioServerTransport()
    .WithTools<Tools>()
    .WithTools<IngestTool>()
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
