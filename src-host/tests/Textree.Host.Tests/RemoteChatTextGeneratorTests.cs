using System.Net;
using System.Text;
using Textree.Host.Rag;
using Xunit;

namespace Textree.Host.Tests;

public sealed class RemoteChatTextGeneratorTests
{
    // Minimal OpenAI-compatible Chat Completions stub server. Empirically (verified against the
    // actual request the ironhive/OpenAI SDK client sends), the client always requests
    // `"stream":true` — RemoteChatTextGenerator only ever calls the streaming API, there is no
    // non-streaming code path to negotiate away from. A stub answering with a single plain JSON
    // completion object (no SSE framing) is silently swallowed by the SDK's SSE parser (zero
    // chunks, no exception) rather than raising an error, so the stub must speak real
    // `text/event-stream` framing (`data: {...}\n\n`, terminated by `data: [DONE]\n\n`) to
    // actually exercise the streaming/response-text path end-to-end.
    private static (HttpListener listener, string baseUrl) StartStub(string assistantText, int statusCode = 200)
    {
        var listener = new HttpListener();
        var port = GetFreePort();
        var baseUrl = $"http://127.0.0.1:{port}";
        listener.Prefixes.Add(baseUrl + "/");
        listener.Start();
        _ = Task.Run(async () =>
        {
            try
            {
                var ctx = await listener.GetContextAsync();
                ctx.Response.StatusCode = statusCode;
                ctx.Response.ContentType = "text/event-stream";
                var chunkJson =
                    $$"""{"id":"x","object":"chat.completion.chunk","created":0,"model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"{{assistantText}}"},"finish_reason":null}]}""";
                await using var writer = new StreamWriter(ctx.Response.OutputStream, Encoding.UTF8) { AutoFlush = true };
                await writer.WriteAsync($"data: {chunkJson}\n\n");
                await writer.WriteAsync("data: [DONE]\n\n");
            }
            catch (HttpListenerException) { /* listener stopped under us — test is done */ }
            catch (ObjectDisposedException) { /* same */ }
        });
        return (listener, baseUrl);
    }

    private static int GetFreePort()
    {
        var l = new System.Net.Sockets.TcpListener(System.Net.IPAddress.Loopback, 0);
        l.Start();
        var port = ((System.Net.IPEndPoint)l.LocalEndpoint).Port;
        l.Stop();
        return port;
    }

    private const string AssistantText = "hello from byo";

    [Fact]
    public async Task GenerateAsync_returns_remote_response_text_for_ollama_preset()
    {
        var (listener, baseUrl) = StartStub(AssistantText);
        try
        {
            var gen = new RemoteChatTextGenerator("ollama", baseUrl, apiKey: null, model: "llama3");
            var chunks = new List<string>();
            await foreach (var chunk in gen.GenerateAsync(
                [new ChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
            {
                chunks.Add(chunk);
            }
            Assert.Contains("hello from byo", string.Concat(chunks));
        }
        finally { listener.Stop(); }
    }

    [Fact]
    public async Task GenerateAsync_returns_remote_response_text_for_gpustack_preset()
    {
        var (listener, baseUrl) = StartStub(AssistantText);
        try
        {
            // GpuStackConfig bakes in the `/v1-openai/` path itself — the stub above answers on
            // any path under baseUrl since HttpListener with a bare-host prefix matches all paths.
            var gen = new RemoteChatTextGenerator("gpustack", baseUrl, apiKey: null, model: "llama3");
            var chunks = new List<string>();
            await foreach (var chunk in gen.GenerateAsync(
                [new ChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
            {
                chunks.Add(chunk);
            }
            Assert.Contains("hello from byo", string.Concat(chunks));
        }
        finally { listener.Stop(); }
    }

    [Fact]
    public async Task GenerateAsync_records_last_error_when_server_unreachable()
    {
        // No listener bound — connection refused within the 2s ConnectTimeout baked into
        // OpenAICompatibleConfig/GpuStackConfig.
        var port = GetFreePort();
        var gen = new RemoteChatTextGenerator("ollama", $"http://127.0.0.1:{port}", apiKey: null, model: "llama3");

        await Assert.ThrowsAnyAsync<Exception>(async () =>
        {
            await foreach (var _ in gen.GenerateAsync(
                [new ChatMessage("user", "hi")], new GenerationOptions(), CancellationToken.None))
            { }
        });

        Assert.NotNull(gen.LastError);
    }

    [Fact]
    public void Ready_is_true_immediately_after_construction()
    {
        // No load step for a remote provider (unlike LocalTextGenerator's model download) —
        // reachability is proven per-request, not up front. Ready means "configured", matching
        // the honest lazy-connect model ironhive's own Config classes use.
        var gen = new RemoteChatTextGenerator("ollama", "http://127.0.0.1:1", apiKey: null, model: "llama3");
        Assert.True(gen.Ready);
    }
}
