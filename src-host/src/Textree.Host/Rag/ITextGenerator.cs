namespace Textree.Host.Rag;

public sealed record ChatMessage(string Role, string Content);
public sealed record GenerationOptions(int MaxTokens = 512, float Temperature = 0.2f);

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
    // Streams token chunks. Honors ct (client disconnect -> stop, free CPU).
    IAsyncEnumerable<string> GenerateAsync(
        IReadOnlyList<ChatMessage> messages, GenerationOptions opts, CancellationToken ct);
}
