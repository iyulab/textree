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
            chunks.Add(chunk);
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

    // ── Fake IChatClient: streams canned updates, or throws, and records Dispose calls ───────
    private sealed class FakeChatClient : IChatClient
    {
        private readonly string[] _updates;
        private readonly string? _throwMessage;

        public FakeChatClient(string[]? updates = null, string? throwMessage = null)
        {
            _updates = updates ?? [];
            _throwMessage = throwMessage;
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
