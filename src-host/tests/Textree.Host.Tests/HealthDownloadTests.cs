using System.Net.Http.Json;
using System.Text.Json;
using LMSupply;
using LMSupply.Embedder;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Textree.Host.Rag;
using Xunit;

// Integration test: verifies that the host begins listening BEFORE the embedder finishes loading.
// The stub loader reports one progress tick then blocks until signalled — the host must respond
// to /health immediately during that window (embedderReady=false, Downloading phase).
public sealed class HealthDownloadTests
{
    [Fact]
    public async Task Health_responds_immediately_while_embedder_is_loading()
    {
        var gate = new TaskCompletionSource<IEmbeddingModel>(TaskCreationOptions.RunContinuationsAsynchronously);
        var stubLoader = new StubEmbedderLoader(
            gate.Task,
            bytesDownloaded: 1_200_000_000L,
            totalBytes: 2_900_000_000L);

        using var factory = new TextreeHostFactory(stubLoader);
        var client = factory.CreateClient();

        // Wait until the stub has reported at least one progress tick — proves
        // the background task is live and the host is listening concurrently.
        await stubLoader.ProgressReported.WaitAsync(TimeSpan.FromSeconds(10));

        // Host must respond immediately — not blocked on the embedder load.
        var health = await client.GetFromJsonAsync<JsonElement>("/health");

        Assert.False(health.GetProperty("embedderReady").GetBoolean());

        // ModelStatus must reflect the Downloading phase and reported bytes.
        var status = factory.Services.GetRequiredService<ModelStatus>();
        var snap = status.Embedder;
        Assert.Equal(ModelPhase.Downloading, snap.Phase);
        Assert.Equal(1_200_000_000L, snap.BytesDownloaded);
        Assert.Equal(2_900_000_000L, snap.TotalBytes);
    }

    // ── Test host: replaces IEmbedderLoader with the stub via ConfigureTestServices ──
    private sealed class TextreeHostFactory : WebApplicationFactory<Program>
    {
        private readonly IEmbedderLoader _loader;

        public TextreeHostFactory(IEmbedderLoader loader) => _loader = loader;

        protected override void ConfigureWebHost(Microsoft.AspNetCore.Hosting.IWebHostBuilder builder)
        {
            builder.ConfigureTestServices(services =>
            {
                services.RemoveAll<IEmbedderLoader>();
                services.AddSingleton(_loader);
            });
        }
    }

    // ── Stub loader: reports one progress tick then blocks until the gate task resolves ──
    private sealed class StubEmbedderLoader : IEmbedderLoader
    {
        private readonly Task<IEmbeddingModel> _gateTask;
        private readonly long _bytesDownloaded;
        private readonly long _totalBytes;
        private readonly SemaphoreSlim _progressReported = new(0, 1);

        public StubEmbedderLoader(Task<IEmbeddingModel> gateTask, long bytesDownloaded, long totalBytes)
        {
            _gateTask = gateTask;
            _bytesDownloaded = bytesDownloaded;
            _totalBytes = totalBytes;
        }

        /// <summary>
        /// Signalled (released) once the stub has reported its first progress tick to the caller.
        /// Await this in the test before hitting /health to guarantee the background task is running.
        /// </summary>
        public SemaphoreSlim ProgressReported => _progressReported;

        public async Task<IEmbeddingModel> LoadAsync(
            string modelId,
            IProgress<DownloadProgress> progress,
            CancellationToken ct)
        {
            // Report one download progress tick.
            progress.Report(new DownloadProgress
            {
                FileName = "model.onnx.data",
                BytesDownloaded = _bytesDownloaded,
                TotalBytes = _totalBytes,
                CurrentFileIndex = 1,
                TotalFileCount = 2,
            });

            // Signal the test that the tick was reported.
            _progressReported.Release();

            // Block until the test completes (simulates a long cold download).
            return await _gateTask;
        }
    }
}
