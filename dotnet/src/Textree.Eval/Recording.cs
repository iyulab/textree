using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Extensions.AI;

namespace Textree.Eval;

/// <summary>
/// A chat client that records each answer under the hash of its question, or answers from such a
/// recording without a model. Replaying a recording makes an evaluation repeatable byte for byte.
/// A question asked more than once (repeated runs) keeps one answer per occurrence, in order.
/// </summary>
public sealed class RecordingChatClient : IChatClient
{
    private readonly IChatClient? _inner;
    private readonly Dictionary<string, List<JsonObject>> _answers;
    private readonly Dictionary<string, int> _cursor = new();
    private readonly string? _saveTo;

    private RecordingChatClient(IChatClient? inner, Dictionary<string, List<JsonObject>> answers, string? saveTo)
    {
        _inner = inner;
        _answers = answers;
        _saveTo = saveTo;
    }

    /// <summary>Asks <paramref name="inner"/> and records every answer, saved to <paramref name="path"/> by <see cref="Save"/>.</summary>
    public static RecordingChatClient Record(IChatClient inner, string path) => new(inner, new(), path);

    /// <summary>Answers from the recording at <paramref name="path"/>; a question it does not hold is an error.</summary>
    public static RecordingChatClient Replay(string path)
    {
        var json = JsonNode.Parse(File.ReadAllText(path))!.AsObject();
        var answers = json.ToDictionary(kv => kv.Key, kv => kv.Value!.AsArray().Select(a => a!.AsObject()).ToList());
        return new(null, answers, null);
    }

    public void Save()
    {
        if (_saveTo is null)
        {
            return;
        }
        var o = new JsonObject();
        foreach (var (k, list) in _answers.OrderBy(kv => kv.Key, StringComparer.Ordinal))
        {
            o[k] = new JsonArray(list.Select(a => (JsonNode)a.DeepClone()).ToArray());
        }
        File.WriteAllText(_saveTo, o.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
    }

    public async Task<ChatResponse> GetResponseAsync(IEnumerable<ChatMessage> messages, ChatOptions? options = null, CancellationToken cancellationToken = default)
    {
        var list = messages.ToList();
        var key = Key(list);
        var index = _cursor.GetValueOrDefault(key);
        _cursor[key] = index + 1;
        if (_inner is null)
        {
            if (!_answers.TryGetValue(key, out var recorded) || recorded.Count == 0)
            {
                throw new InvalidOperationException("this question is not in the recording");
            }
            var a = recorded[Math.Min(index, recorded.Count - 1)];
            return new ChatResponse(new ChatMessage(ChatRole.Assistant, a["text"]!.GetValue<string>()))
            {
                Usage = new UsageDetails { InputTokenCount = a["input"]?.GetValue<long>(), OutputTokenCount = a["output"]?.GetValue<long>() },
            };
        }
        var response = await _inner.GetResponseAsync(list, options, cancellationToken);
        lock (_answers)
        {
            if (!_answers.TryGetValue(key, out var answers))
            {
                _answers[key] = answers = [];
            }
            answers.Add(new JsonObject
            {
                ["text"] = response.Text,
                ["input"] = response.Usage?.InputTokenCount,
                ["output"] = response.Usage?.OutputTokenCount,
            });
        }
        return response;
    }

    public IAsyncEnumerable<ChatResponseUpdate> GetStreamingResponseAsync(IEnumerable<ChatMessage> messages, ChatOptions? options = null, CancellationToken cancellationToken = default) =>
        throw new NotSupportedException();

    public object? GetService(Type serviceType, object? serviceKey = null) => _inner?.GetService(serviceType, serviceKey);

    public void Dispose() => _inner?.Dispose();

    private static string Key(List<ChatMessage> messages)
    {
        var sb = new StringBuilder();
        foreach (var m in messages)
        {
            sb.Append(m.Role.Value).Append('\u0001').Append(m.Text).Append('\u0002');
        }
        return Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(sb.ToString())), 0, 16).ToLowerInvariant();
    }
}
