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
/// Recording into a file that already holds answers continues it: questions it holds are answered
/// from it, new ones are asked and added, and the file is saved as it grows — an evaluation that
/// stops part way loses at most the last few answers and resumes where it was.
/// Each answer keeps the seconds the model took, so a replay reports the time of the run that was
/// recorded (<see cref="ReplayedSeconds"/>) rather than the time of reading the file.
/// </summary>
public sealed class RecordingChatClient : IChatClient
{
    private readonly IChatClient? _inner;
    private readonly Dictionary<string, List<JsonObject>> _answers;
    private readonly Dictionary<string, int> _cursor = new();
    private readonly string? _saveTo;
    private readonly object _writing = new();
    private DateTime _savedAt = DateTime.UtcNow;
    private static readonly TimeSpan SaveEvery = TimeSpan.FromSeconds(20);
    private long _replayedTicks;
    private int _replayed;
    private int _unrecordedTimes;

    /// <summary>Seconds the model took, as recorded, for every answer replayed so far.</summary>
    public double ReplayedSeconds => TimeSpan.FromTicks(Interlocked.Read(ref _replayedTicks)).TotalSeconds;

    /// <summary>Answers replayed from the recording so far.</summary>
    public int Replayed => Volatile.Read(ref _replayed);

    /// <summary>Replayed answers whose recording holds no time (made before times were kept).</summary>
    public int UnrecordedTimes => Volatile.Read(ref _unrecordedTimes);

    private RecordingChatClient(IChatClient? inner, Dictionary<string, List<JsonObject>> answers, string? saveTo)
    {
        _inner = inner;
        _answers = answers;
        _saveTo = saveTo;
    }

    /// <summary>
    /// Asks <paramref name="inner"/> and records every answer to <paramref name="path"/>; if that file
    /// already holds a recording, continues it.
    /// </summary>
    public static RecordingChatClient Record(IChatClient inner, string path) =>
        new(inner, File.Exists(path) ? Load(path) : new(), path);

    /// <summary>Answers from the recording at <paramref name="path"/>; a question it does not hold is an error.</summary>
    public static RecordingChatClient Replay(string path) => new(null, Load(path), null);

    private static Dictionary<string, List<JsonObject>> Load(string path)
    {
        var json = JsonNode.Parse(File.ReadAllText(path))!.AsObject();
        return json.ToDictionary(kv => kv.Key, kv => kv.Value!.AsArray().Select(a => a!.AsObject()).ToList());
    }

    public void Save()
    {
        if (_saveTo is null)
        {
            return;
        }
        string text;
        lock (_answers)
        {
            var o = new JsonObject();
            foreach (var (k, list) in _answers.OrderBy(kv => kv.Key, StringComparer.Ordinal))
            {
                o[k] = new JsonArray(list.Select(a => (JsonNode)a.DeepClone()).ToArray());
            }
            text = o.ToJsonString(new JsonSerializerOptions { WriteIndented = true });
            _savedAt = DateTime.UtcNow;
        }
        // Written beside and moved over, so a stop during the write leaves the last whole recording.
        lock (_writing)
        {
            var temp = _saveTo + ".tmp";
            File.WriteAllText(temp, text);
            File.Move(temp, _saveTo, overwrite: true);
        }
    }

    public async Task<ChatResponse> GetResponseAsync(IEnumerable<ChatMessage> messages, ChatOptions? options = null, CancellationToken cancellationToken = default)
    {
        var list = messages.ToList();
        var key = Key(list);
        JsonObject? held = null;
        lock (_answers)
        {
            var index = _cursor.GetValueOrDefault(key);
            _cursor[key] = index + 1;
            if (_answers.TryGetValue(key, out var recorded) && recorded.Count > 0)
            {
                if (_inner is null)
                {
                    held = recorded[Math.Min(index, recorded.Count - 1)];
                }
                else if (index < recorded.Count)
                {
                    held = recorded[index];
                }
            }
        }
        if (held is not null)
        {
            Interlocked.Increment(ref _replayed);
            if (held["seconds"] is { } recordedSeconds)
            {
                Interlocked.Add(ref _replayedTicks, TimeSpan.FromSeconds(recordedSeconds.GetValue<double>()).Ticks);
            }
            else
            {
                Interlocked.Increment(ref _unrecordedTimes);
            }
            return new ChatResponse(new ChatMessage(ChatRole.Assistant, held["text"]!.GetValue<string>()))
            {
                Usage = new UsageDetails { InputTokenCount = held["input"]?.GetValue<long>(), OutputTokenCount = held["output"]?.GetValue<long>() },
            };
        }
        if (_inner is null)
        {
            throw new InvalidOperationException("this question is not in the recording");
        }
        var clock = System.Diagnostics.Stopwatch.StartNew();
        var response = await _inner.GetResponseAsync(list, options, cancellationToken);
        var took = clock.Elapsed;
        bool due;
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
                ["seconds"] = Math.Round(took.TotalSeconds, 3),
            });
            due = _saveTo is not null && DateTime.UtcNow - _savedAt >= SaveEvery;
            if (due)
            {
                _savedAt = DateTime.UtcNow;
            }
        }
        if (due)
        {
            Save();
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
