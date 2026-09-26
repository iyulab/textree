namespace Textree.Host.Rag;

public sealed record ChatMessage(string Role, string Content);
public sealed record GenerationOptions(int MaxTokens = 512, float Temperature = 0.2f);

// One piece of a streamed answer. FinishReason is set only on the last piece, when the backend says
// why the answer ended: "stop" (it was done), "length" (it reached the token cap), "degeneration"
// (it fell into repeating itself and was stopped). Null when not known.
public sealed record GenerationChunk(string? Text, string? FinishReason = null);

public interface ITextGenerator
{
    bool Ready { get; }
    // Non-null when the most recent PrepareAsync attempt failed (model download/load error).
    // Cleared at the start of a fresh attempt. Surfaced via /health so the desktop app can tell
    // "still preparing" apart from "failed" instead of hanging on a permanent preparing state.
    string? LastError { get; }
    // What the loaded model actually runs on (e.g. "CPU", "DirectML"), as the runtime reports it;
    // null until loaded, or when generation happens elsewhere. Lets /health say which hardware
    // path was taken rather than which one was asked for.
    IReadOnlyList<string>? ActiveProviders => null;
    // Lazily loads the model on first call if not yet loaded.
    Task PrepareAsync(CancellationToken ct);
    // Streams token chunks, the last one saying why the answer ended when that is known.
    // Honors ct (client disconnect -> stop, free CPU).
    IAsyncEnumerable<GenerationChunk> GenerateAsync(
        IReadOnlyList<ChatMessage> messages, GenerationOptions opts, CancellationToken ct);
}
