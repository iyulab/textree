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

        using var factory = new TextreeHostFactory(loader: stubLoader);
        var client = factory.CreateClient();

        // Wait until the stub has reported at least one progress tick — proves
        // the background task is live and the host is listening concurrently.
        var progressReported = await stubLoader.ProgressReported.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.True(progressReported, "Stub loader did not report progress within 10s — background task may be blocked.");

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

    [Fact]
    public async Task Health_includes_generator_download_snapshot()
    {
        // Build a ModelStatus that already has a generator download in progress.
        var modelStatus = new ModelStatus();
        modelStatus.SetGeneratorPhase(ModelPhase.Downloading);
        modelStatus.GeneratorProgress.Report(new DownloadProgress
        {
            FileName = "model.gguf",
            BytesDownloaded = 300_000_000,
            TotalBytes = 2_200_000_000,
            CurrentFileIndex = 1,
            TotalFileCount = 2,
        });

        // Use a never-resolving loader for the background embedder task so the test stays
        // hermetic and the background download does not interfere with assertions.
        var embedderGate = new TaskCompletionSource<IEmbeddingModel>(TaskCreationOptions.RunContinuationsAsynchronously);
        var embedderLoader = new StubEmbedderLoader(embedderGate.Task, 0, 0);

        using var app = new TextreeHostFactory(modelStatus: modelStatus, loader: embedderLoader);
        var client = app.CreateClient();

        var health = await client.GetFromJsonAsync<JsonElement>("/health");
        var dl = health.GetProperty("generatorDownload");
        Assert.Equal("downloading", dl.GetProperty("phase").GetString());
        Assert.Equal(300_000_000, dl.GetProperty("bytesDownloaded").GetInt64());
        Assert.Equal(2, dl.GetProperty("fileCount").GetInt32());
    }

    // ── Test host: replaces IEmbedderLoader and/or ModelStatus with stubs ──
    private sealed class TextreeHostFactory : WebApplicationFactory<Program>
    {
        private readonly IEmbedderLoader? _loader;
        private readonly ModelStatus? _modelStatus;

        /// <summary>Constructor for embedder-loading tests (original use case).</summary>
        public TextreeHostFactory(IEmbedderLoader loader) => _loader = loader;

        /// <summary>
        /// Constructor for generator-focused tests: injects a custom <see cref="ModelStatus"/>
        /// and a custom loader so the background embedder task does not interfere.
        /// </summary>
        public TextreeHostFactory(ModelStatus modelStatus, IEmbedderLoader loader)
        {
            _modelStatus = modelStatus;
            _loader = loader;
        }

        protected override void ConfigureWebHost(Microsoft.AspNetCore.Hosting.IWebHostBuilder builder)
        {
            builder.ConfigureTestServices(services =>
            {
                if (_loader is not null)
                {
                    services.RemoveAll<IEmbedderLoader>();
                    services.AddSingleton(_loader);
                }

                if (_modelStatus is not null)
                {
                    services.RemoveAll<ModelStatus>();
                    services.AddSingleton(_modelStatus);
                }
            });
        }
    }

    // ── Stub loader: optionally reports one progress tick then blocks until gate resolves ──
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
            if (_bytesDownloaded > 0 || _totalBytes > 0)
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
            }

            // Signal the test that the tick was reported (or that LoadAsync was entered).
            _progressReported.Release();

            // Block until the gate resolves (simulates a long cold download or an idle wait).
            return await _gateTask;
        }
    }
}
