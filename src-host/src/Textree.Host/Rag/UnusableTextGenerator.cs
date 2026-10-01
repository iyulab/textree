using System.Runtime.CompilerServices;

namespace Textree.Host.Rag;

/// <summary>
/// Stands in for a BYO endpoint whose settings cannot be used (a frontier preset without a key, a
/// custom server without an address). It never becomes ready and reports why through
/// <see cref="LastError"/>, so /health tells the desktop app the configuration failed instead of the
/// first chat request failing with a provider error.
/// </summary>
public sealed class UnusableTextGenerator(string reason) : ITextGenerator
{
    public bool Ready => false;
    public string? LastError => reason;

    public Task PrepareAsync(CancellationToken ct) => Task.CompletedTask;

    public async IAsyncEnumerable<GenerationChunk> GenerateAsync(
        IReadOnlyList<ChatMessage> messages,
        GenerationOptions opts,
        [EnumeratorCancellation] CancellationToken ct)
    {
        await Task.CompletedTask;
        throw new InvalidOperationException(reason);
#pragma warning disable CS0162 // an iterator needs a yield to be one
        yield break;
#pragma warning restore CS0162
    }
}
