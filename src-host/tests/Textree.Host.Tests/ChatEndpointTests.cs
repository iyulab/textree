using System.Net;
using System.Net.Http.Json;
using System.Runtime.CompilerServices;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Textree.Host.Rag;
using Xunit;

// SSE-format and cancellation tests for POST /chat. These run against a STUB ITextGenerator
// (DI-injected) so they do NOT depend on a downloaded text-generation model. The embedder
// still loads at host startup from its on-disk cache (multilingual-e5-small), which is the
// existing host behavior and not under test here.
public sealed class ChatEndpointTests
{
    [Fact]
    public async Task Chat_streams_sse_chunks_then_done()
    {
        var stub = new StubGenerator(chunks: ["Hel", "lo"]);
        using var factory = new Factory(stub);
        var client = factory.CreateClient();

        var resp = await client.PostAsJsonAsync("/chat",
            new { messages = new[] { new { role = "user", content = "hi" } }, stream = true });

        Assert.Equal(HttpStatusCode.OK, resp.StatusCode);
        Assert.Equal("text/event-stream", resp.Content.Headers.ContentType!.MediaType);

        var body = await resp.Content.ReadAsStringAsync();

        Assert.Contains("\"delta\":{\"content\":\"Hel\"}", body);
        Assert.Contains("\"delta\":{\"content\":\"lo\"}", body);
        Assert.EndsWith("data: [DONE]\n\n", body);
    }

    [Fact]
    public async Task Chat_rejects_empty_messages()
    {
        var stub = new StubGenerator(chunks: ["x"]);
        using var factory = new Factory(stub);
        var client = factory.CreateClient();

        var resp = await client.PostAsJsonAsync("/chat",
            new { messages = Array.Empty<object>(), stream = true });

        Assert.Equal(HttpStatusCode.BadRequest, resp.StatusCode);
    }

    [Fact]
    public async Task Chat_forwards_max_tokens_to_generator()
    {
        // Regression for the ONNX containment fix: verify that req.MaxTokens (and the ?? 512
        // default) is propagated into GenerationOptions so the cap actually reaches the backend.
        var stub = new CapturingGenerator(chunks: ["ok"]);
        using var factory = new Factory(stub);
        var client = factory.CreateClient();

        // Explicit value: expect it forwarded verbatim.
        await client.PostAsJsonAsync("/chat",
            new { messages = new[] { new { role = "user", content = "hi" } }, maxTokens = 64, stream = true });
        Assert.Equal(64, stub.LastMaxTokens);

        // Omitted value: expect the ?? 512 default applied by the endpoint.
        stub.Reset();
        await client.PostAsJsonAsync("/chat",
            new { messages = new[] { new { role = "user", content = "hi" } }, stream = true });
        Assert.Equal(512, stub.LastMaxTokens);
    }

    [Fact]
    public async Task Health_reports_generator_ready_flag()
    {
        var stub = new StubGenerator(chunks: ["x"]);
        using var factory = new Factory(stub);
        var client = factory.CreateClient();

        var body = await client.GetStringAsync("/health");

        Assert.Contains("generatorReady", body);
    }

    [Fact]
    public async Task Prepare_generation_is_idempotent()
    {
        var stub = new StubGenerator(chunks: ["x"]);
        using var factory = new Factory(stub);
        var client = factory.CreateClient();

        var resp1 = await client.PostAsync("/prepare-generation", null);
        var resp2 = await client.PostAsync("/prepare-generation", null);

        Assert.Equal(System.Net.HttpStatusCode.Accepted, resp1.StatusCode);
        Assert.Equal(System.Net.HttpStatusCode.Accepted, resp2.StatusCode);
    }

    [Fact]
    public async Task Chat_stops_generating_when_client_cancels()
    {
        // Stub blocks after the first chunk until its ct is cancelled, then records it.
        var stub = new StubGenerator(chunks: ["first"], blockAfterFirst: true);
        using var factory = new Factory(stub);
        var client = factory.CreateClient();

        using var cts = new CancellationTokenSource();
        var req = new HttpRequestMessage(HttpMethod.Post, "/chat")
        {
            Content = JsonContent.Create(
                new { messages = new[] { new { role = "user", content = "hi" } }, stream = true }),
        };

        // Start streaming; read headers only. The generator is now mid-stream, blocked after
        // emitting its first chunk and awaiting cancellation on the endpoint's ct.
        var resp = await client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, cts.Token);

        // Wait until the generator has actually started (first chunk emitted) before aborting,
        // so the cancel is genuinely mid-stream and not a no-op before generation began.
        Assert.True(await stub.WaitForStartAsync(TimeSpan.FromSeconds(5)));

        // Abort the request (client disconnect). The endpoint's ct = HttpContext.RequestAborted
        // must fire and propagate into the generator, stopping it and freeing the CPU.
        cts.Cancel();
        resp.Dispose();

        Assert.True(await stub.WaitForCancellationAsync(TimeSpan.FromSeconds(5)));
    }

    // ── Test host: swaps the real generator for a stub via ConfigureTestServices ──────
    public sealed class Factory : WebApplicationFactory<Program>
    {
        private readonly ITextGenerator _generator;

        public Factory(ITextGenerator generator) => _generator = generator;

        protected override void ConfigureWebHost(Microsoft.AspNetCore.Hosting.IWebHostBuilder builder)
        {
            builder.ConfigureTestServices(services =>
            {
                services.RemoveAll<ITextGenerator>();
                services.AddSingleton(_generator);
            });
        }
    }

    // ── Capturing generator: records GenerationOptions for assertion ─────────────────
    private sealed class CapturingGenerator : ITextGenerator
    {
        private readonly string[] _chunks;
        private int? _lastMaxTokens;

        public CapturingGenerator(string[] chunks) => _chunks = chunks;

        public bool Ready => true;
        public Task PrepareAsync(CancellationToken ct) => Task.CompletedTask;

        /// <summary>MaxTokens from the most recent <see cref="GenerateAsync"/> call.</summary>
        public int? LastMaxTokens => _lastMaxTokens;

        public void Reset() => _lastMaxTokens = null;

        public async IAsyncEnumerable<string> GenerateAsync(
            IReadOnlyList<ChatMessage> messages,
            GenerationOptions opts,
            [EnumeratorCancellation] CancellationToken ct)
        {
            _lastMaxTokens = opts.MaxTokens;
            foreach (var chunk in _chunks)
            {
                ct.ThrowIfCancellationRequested();
                yield return chunk;
                await Task.Yield();
            }
        }
    }

    // ── Stub generator: canned chunks; optionally blocks until its ct cancels ────────
    private sealed class StubGenerator : ITextGenerator
    {
        private readonly string[] _chunks;
        private readonly bool _blockAfterFirst;
        private readonly TaskCompletionSource _started =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource _cancelled =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public StubGenerator(string[] chunks, bool blockAfterFirst = false)
        {
            _chunks = chunks;
            _blockAfterFirst = blockAfterFirst;
        }

        public bool Ready => true;

        public Task PrepareAsync(CancellationToken ct) => Task.CompletedTask;

        public async IAsyncEnumerable<string> GenerateAsync(
            IReadOnlyList<ChatMessage> messages,
            GenerationOptions opts,
            [EnumeratorCancellation] CancellationToken ct)
        {
            for (var i = 0; i < _chunks.Length; i++)
            {
                ct.ThrowIfCancellationRequested();
                yield return _chunks[i];
                _started.TrySetResult();

                if (_blockAfterFirst && i == 0)
                {
                    // Register cancellation BEFORE awaiting so we never miss the signal.
                    using var reg = ct.Register(() => _cancelled.TrySetResult());
                    try
                    {
                        await Task.Delay(Timeout.Infinite, ct);
                    }
                    catch (OperationCanceledException)
                    {
                        _cancelled.TrySetResult();
                        throw;
                    }
                }
            }
        }

        public async Task<bool> WaitForStartAsync(TimeSpan timeout)
        {
            var done = await Task.WhenAny(_started.Task, Task.Delay(timeout));
            return done == _started.Task;
        }

        public async Task<bool> WaitForCancellationAsync(TimeSpan timeout)
        {
            var done = await Task.WhenAny(_cancelled.Task, Task.Delay(timeout));
            return done == _cancelled.Task;
        }
    }
}
