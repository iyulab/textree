using System.ClientModel;
using Microsoft.Extensions.AI;
using OpenAI;

namespace Textree.Eval;

/// <summary>An OpenAI-compatible chat model; the key, if any, from TEXTREE_LLM_KEY.</summary>
public static class Llm
{
    /// <param name="temperature">Sent with every call that does not set its own; null sends none (some models accept only their default).</param>
    public static IChatClient Connect(string endpoint, string model, string? keyVariable = null, float? temperature = 0)
    {
        var key = Environment.GetEnvironmentVariable(keyVariable ?? "TEXTREE_LLM_KEY");
        var client = new OpenAIClient(new ApiKeyCredential(string.IsNullOrEmpty(key) ? "none" : key), new OpenAIClientOptions { Endpoint = new Uri(endpoint), NetworkTimeout = TimeSpan.FromMinutes(5) });
        var chat = client.GetChatClient(model).AsIChatClient();
        return temperature is null ? chat : new WithTemperature(chat, temperature.Value);
    }
}

/// <summary>Sends a temperature with every call that does not set its own.</summary>
internal sealed class WithTemperature(IChatClient inner, float temperature) : DelegatingChatClient(inner)
{
    public override Task<ChatResponse> GetResponseAsync(IEnumerable<ChatMessage> messages, ChatOptions? options = null, CancellationToken cancellationToken = default) =>
        base.GetResponseAsync(messages, With(options), cancellationToken);

    public override IAsyncEnumerable<ChatResponseUpdate> GetStreamingResponseAsync(IEnumerable<ChatMessage> messages, ChatOptions? options = null, CancellationToken cancellationToken = default) =>
        base.GetStreamingResponseAsync(messages, With(options), cancellationToken);

    private ChatOptions With(ChatOptions? options)
    {
        var o = options?.Clone() ?? new ChatOptions();
        o.Temperature ??= temperature;
        return o;
    }
}
