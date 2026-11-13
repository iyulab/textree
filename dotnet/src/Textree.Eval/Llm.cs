using System.ClientModel;
using Microsoft.Extensions.AI;
using OpenAI;

namespace Textree.Eval;

/// <summary>An OpenAI-compatible chat model; the key, if any, from TEXTREE_LLM_KEY.</summary>
public static class Llm
{
    public static IChatClient Connect(string endpoint, string model, string? keyVariable = null)
    {
        var key = Environment.GetEnvironmentVariable(keyVariable ?? "TEXTREE_LLM_KEY");
        var client = new OpenAIClient(new ApiKeyCredential(string.IsNullOrEmpty(key) ? "none" : key), new OpenAIClientOptions { Endpoint = new Uri(endpoint), NetworkTimeout = TimeSpan.FromMinutes(5) });
        return client.GetChatClient(model).AsIChatClient();
    }
}
