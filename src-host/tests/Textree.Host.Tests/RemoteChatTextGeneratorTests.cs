using System.Runtime.CompilerServices;
using Microsoft.Extensions.AI;
using Textree.Host.Rag;
using Xunit;
using MeaiChatMessage = Microsoft.Extensions.AI.ChatMessage;
using RagChatMessage = Textree.Host.Rag.ChatMessage;

namespace Textree.Host.Tests;

// RemoteChatTextGenerator no longer constructs providers — Program.cs registers exactly one
// iron-prow provider (ByoProviderRegistration, covered by ByoProviderRegistrationTests) and this
// class only adapts the resulting DI-singleton IChatClient into ITextGenerator. So these tests
// exercise streaming behavior in isolation against a hand-rolled fake IChatClient (the test
// project references no mocking library — see Textree.Host.Tests.csproj). Preset→provider dispatch
// is out of scope here; it moved to ByoProviderRegistration.
public sealed class RemoteChatTextGeneratorTests
{
    private const string Model = "llama3";

    [Fact]
    public async Task GenerateAsync_streams_concatenated_text_from_injected_client()
    {
        var fake = new FakeChatClient(updates: ["hello ", "from ", "byo"]);
        var gen = new RemoteChatTextGenerator(fake, Model);

        var chunks = new List<string>();
        await foreach (var chunk in gen.GenerateAsync(
            [new RagChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
        {
            if (chunk.Text is { } text) chunks.Add(text);
        }

        Assert.Equal("hello from byo", string.Concat(chunks));
    }

    [Fact]
    public async Task GenerateAsync_records_last_error_and_rethrows_when_client_throws()
    {
        var fake = new FakeChatClient(throwMessage: "connection refused");
        var gen = new RemoteChatTextGenerator(fake, Model);

        var ex = await Assert.ThrowsAnyAsync<Exception>(async () =>
        {
            await foreach (var _ in gen.GenerateAsync(
                [new RagChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
            { }
        });

        Assert.Equal("connection refused", ex.Message);
        Assert.Equal("connection refused", gen.LastError);
    }

    [Fact]
    public void Ready_is_true_immediately_after_construction()
    {
        // No load step for a remote provider (unlike LocalTextGenerator's model download):
        // reachability is proven per-request, not up front. Ready means "configured".
        var gen = new RemoteChatTextGenerator(new FakeChatClient(updates: []), Model);
        Assert.True(gen.Ready);
    }

    [Fact]
    public async Task GenerateAsync_never_disposes_the_injected_singleton_client()
    {
        // Regression lock for the singleton-disposal bug: the injected IChatClient is a DI
        // singleton (iron-prow's gateway, shared across every /chat request). The generator wraps
        // it in a LocalSafetyChatClient (a DelegatingChatClient, whose Dispose forwards to the
        // inner client), so a `using`/Dispose on that wrapper would dispose the singleton after
        // the first request and break every subsequent one. Assert Dispose is never called, even
        // across multiple GenerateAsync calls.
        var fake = new FakeChatClient(updates: ["a", "b"]);
        var gen = new RemoteChatTextGenerator(fake, Model);

        for (var i = 0; i < 2; i++)
        {
            await foreach (var _ in gen.GenerateAsync(
                [new RagChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
            { }
        }

        Assert.Equal(0, fake.DisposeCallCount);
    }

    [Fact]
    public async Task GenerateAsync_stops_a_degenerate_repetition_loop_before_the_stream_ends()
    {
        // The detection rules themselves are iron-prow's (DegenerationDetector) and tested there.
        // This locks the wiring: the generator is wrapped, so a word loop ends early instead of
        // streaming to the token cap.
        var fake = new FakeChatClient(updates: ["Answer: ", .. Enumerable.Repeat("concisely ", 40)]);
        var gen = new RemoteChatTextGenerator(fake, Model);

        var chunks = new List<string>();
        await foreach (var chunk in gen.GenerateAsync(
            [new RagChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
        {
            if (chunk.Text is { } text) chunks.Add(text);
        }

        Assert.StartsWith("Answer: concisely ", string.Concat(chunks));
        Assert.True(chunks.Count < 41, $"expected an early stop, streamed {chunks.Count} of 41 chunks");
    }

    [Fact]
    public async Task GenerateAsync_says_an_answer_stopped_for_repeating_itself()
    {
        var fake = new FakeChatClient(updates: ["Answer: ", .. Enumerable.Repeat("concisely ", 40)]);
        var gen = new RemoteChatTextGenerator(fake, Model);

        GenerationChunk? last = null;
        await foreach (var chunk in gen.GenerateAsync(
            [new RagChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
        {
            last = chunk;
        }

        Assert.Equal("degeneration", last?.FinishReason);
    }

    [Fact]
    public async Task GenerateAsync_passes_on_why_the_backend_ended_the_answer()
    {
        var gen = new RemoteChatTextGenerator(new FakeChatClient(updates: ["done"], finish: "length"), Model);

        var chunks = new List<GenerationChunk>();
        await foreach (var chunk in gen.GenerateAsync(
            [new RagChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
        {
            chunks.Add(chunk);
        }

        Assert.Equal("done", string.Concat(chunks.Select(c => c.Text)));
        Assert.Equal("length", chunks[^1].FinishReason);
        Assert.All(chunks[..^1], c => Assert.Null(c.FinishReason));
    }

    [Fact]
    public async Task GenerateAsync_lets_markdown_structure_runs_through()
    {
        string[] table = ["| a | b |\n", "| ", .. Enumerable.Repeat("--- | ", 12), "\n| 1 | 2 |"];
        var gen = new RemoteChatTextGenerator(new FakeChatClient(updates: table), Model);

        var chunks = new List<string>();
        await foreach (var chunk in gen.GenerateAsync(
            [new RagChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
        {
            if (chunk.Text is { } text) chunks.Add(text);
        }

        Assert.Equal(string.Concat(table), string.Concat(chunks));
    }

    // ── Fake IChatClient: streams canned updates, or throws, and records Dispose calls ───────
    private sealed class FakeChatClient : IChatClient
    {
        private readonly string[] _updates;
        private readonly string? _throwMessage;
        private readonly string? _finish;

        public FakeChatClient(string[]? updates = null, string? throwMessage = null, string? finish = null)
        {
            _updates = updates ?? [];
            _throwMessage = throwMessage;
            _finish = finish;
        }

        /// <summary>Number of times <see cref="Dispose"/> was called — must stay 0 in use.</summary>
        public int DisposeCallCount { get; private set; }

        public async IAsyncEnumerable<ChatResponseUpdate> GetStreamingResponseAsync(
            IEnumerable<MeaiChatMessage> messages,
            ChatOptions? options = null,
            [EnumeratorCancellation] CancellationToken cancellationToken = default)
        {
            if (_throwMessage is not null)
                throw new InvalidOperationException(_throwMessage);

            foreach (var text in _updates)
            {
                cancellationToken.ThrowIfCancellationRequested();
                yield return new ChatResponseUpdate(ChatRole.Assistant, text);
                await Task.Yield();
            }
            if (_finish is not null)
                yield return new ChatResponseUpdate { Role = ChatRole.Assistant, FinishReason = new ChatFinishReason(_finish) };
        }

        public Task<ChatResponse> GetResponseAsync(
            IEnumerable<MeaiChatMessage> messages,
            ChatOptions? options = null,
            CancellationToken cancellationToken = default) =>
            throw new NotSupportedException("RemoteChatTextGenerator only uses the streaming API.");

        public object? GetService(Type serviceType, object? serviceKey = null) => null;

        public void Dispose() => DisposeCallCount++;
    }
}
