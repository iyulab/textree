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
        IChatClient patient = new Patient(chat);
        return temperature is null ? patient : new WithTemperature(patient, temperature.Value);
    }
}

/// <summary>
/// Asks again, after a pause, when the server fails in a way that may pass (a 5xx answer, 429, a
/// dropped connection or a timeout). One passing failure must not end an evaluation of hours.
/// </summary>
public sealed class Patient(IChatClient inner, int tries = 5) : DelegatingChatClient(inner)
{
    public override async Task<ChatResponse> GetResponseAsync(IEnumerable<ChatMessage> messages, ChatOptions? options = null, CancellationToken cancellationToken = default)
    {
        var list = messages as IList<ChatMessage> ?? messages.ToList();
        for (var attempt = 1; ; attempt++)
        {
            try
            {
                return await base.GetResponseAsync(list, options, cancellationToken);
            }
            catch (Exception e) when (attempt < tries && Passing(e) && !cancellationToken.IsCancellationRequested)
            {
                Console.Error.WriteLine($"model call failed ({e.Message.Split('\n')[0]}); asking again ({attempt}/{tries - 1})");
                await Task.Delay(TimeSpan.FromSeconds(5 * attempt), cancellationToken);
            }
        }
    }

    public static bool Passing(Exception e) => e switch
    {
        ClientResultException c => c.Status is 0 or 408 or 429 || c.Status >= 500,
        HttpRequestException or TimeoutException or IOException => true,
        TaskCanceledException => true,
        _ => e.InnerException is { } inner && Passing(inner),
    };
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
