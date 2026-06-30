using System.Text.Json;
using LMSupply.Embedder;
using Textree.Host;
using Textree.Host.Contracts;
using Textree.Host.Rag;
using Textree.Host.Telemetry;

// ── DI / builder ──────────────────────────────────────────────────────────────
// The embedder loads in the background so the host begins accepting requests
// immediately. Rust's Plan B lifecycle (poll /health with retry on connection-
// refused) still works on cold download; now /health answers promptly with
// embedderReady=false rather than remaining unreachable until the model is on disk.
var opts = new TextreeHostOptions();
var status = new ModelStatus();
// Empty embedder: Dimensions==0 until SetModel is called, keeping EmbedderReady==false
// and /search returning "warming" until the background load finishes.
var embedder = new LmSupplyEmbeddingService();

var builder = WebApplication.CreateBuilder(args);
builder.Services.AddSingleton(opts);
builder.Services.AddSingleton(status);
builder.Services.AddSingleton(embedder);
// Telemetry: disabled (no-op) unless TEXTREE_TELEMETRY_CONNECTION is set. Hand-built pipe
// (allowlist processor + scrub initializer) lives entirely inside TelemetryEmitter.Create.
var telemetryOptions = TelemetryOptions.Read(
    Environment.GetEnvironmentVariables()
        .Cast<System.Collections.DictionaryEntry>()
        .ToDictionary(e => (string)e.Key, e => (string?)(e.Value?.ToString())));
builder.Services.AddSingleton(telemetryOptions);
builder.Services.AddSingleton<ITelemetryEmitter>(sp =>
    TelemetryEmitter.Create(
        telemetryOptions,
        sp.GetRequiredService<ILoggerFactory>().CreateLogger("Textree.Host.Telemetry"),
        EnvFacts.Current()));
builder.Services.AddSingleton<IEmbedderLoader, DefaultEmbedderLoader>();
builder.Services.AddSingleton<VaultManager>();
// Local CPU text generator for /chat. Singleton: the model loads once (lazily, on first
// /chat) and is reused. Tests replace this with a stub via ConfigureTestServices.
builder.Services.AddSingleton<ITextGenerator, LocalTextGenerator>();

var app = builder.Build();

// Background embedder load — host starts listening immediately.
// The background task resolves IEmbedderLoader from the DI container so that
// tests can inject a stub via ConfigureTestServices and have it take effect here.
status.SetEmbedderPhase(ModelPhase.Downloading);
_ = Task.Run(async () =>
{
    try
    {
        var model = await app.Services.GetRequiredService<IEmbedderLoader>()
            .LoadAsync(opts.EmbeddingModel, status.EmbedderProgress, CancellationToken.None);
        status.SetEmbedderPhase(ModelPhase.Loading);
        embedder.SetModel(model);
        status.SetEmbedderPhase(ModelPhase.Ready);
    }
    catch (Exception ex)
    {
        var phaseAtFailure = status.Embedder.Phase; // Downloading/Loading/... before we flip to Error
        status.SetEmbedderError(ex.Message);        // local-only; message keeps the path (data sovereignty OK locally)
        app.Services.GetRequiredService<ITelemetryEmitter>()
            .ReportError(TelemetryPayload.EventForModelFailure(phaseAtFailure), "embedder", phaseAtFailure, ex);
    }
});

var mgr = app.Services.GetRequiredService<VaultManager>();
var reindexLog = app.Services.GetRequiredService<ILoggerFactory>().CreateLogger("Textree.Host.Reindex");
var genLog = app.Services.GetRequiredService<ILoggerFactory>().CreateLogger("Textree.Host.Generation");

// ── Endpoints ─────────────────────────────────────────────────────────────────

app.MapGet("/health", (ITextGenerator gen, ModelStatus modelStatus) =>
{
    static object? Dl(ModelSnapshot s) =>
        s.Phase is ModelPhase.Downloading or ModelPhase.Loading
            ? new
            {
                phase = s.Phase == ModelPhase.Loading ? "loading" : "downloading",
                overallPercent = s.OverallPercent,
                bytesDownloaded = s.BytesDownloaded,
                totalBytes = s.TotalBytes,
                fileIndex = s.FileIndex,
                fileCount = s.FileCount,
            }
            : null;
    return Results.Ok(new
    {
        status = "ok",
        embedderReady = mgr.EmbedderReady,
        generatorReady = gen.Ready,
        generatorError = gen.LastError,
        embedderError = modelStatus.Embedder.Error,
        embedderDownload = Dl(modelStatus.Embedder),
        generatorDownload = Dl(modelStatus.Generator),
    });
});

app.MapPost("/prepare-generation", (ITextGenerator gen) =>
{
    // Idempotent: kick off model load in the background, return immediately.
    // PrepareAsync is internally idempotent (double-checked SemaphoreSlim), so
    // concurrent or repeated calls are safe.
    _ = Task.Run(async () =>
    {
        try { await gen.PrepareAsync(CancellationToken.None); }
        catch (Exception ex)
        {
            genLog.LogError(ex, "Generation model PrepareAsync failed");
            app.Services.GetRequiredService<ITelemetryEmitter>()
                .ReportError(TelemetryEventName.GenerationPrepareFailed, "generator", ModelPhase.Error, ex);
        }
    });
    return Results.Accepted();
});

app.MapPost("/index", async (IndexRequest req, CancellationToken ct) =>
{
    // Boundary validation: empty paths would reach Path.GetFullPath("") and silently
    // resolve to CWD — a scope-correctness hazard. Reject at the edge.
    if (string.IsNullOrWhiteSpace(req.VaultPath) || string.IsNullOrWhiteSpace(req.Path))
        return Results.BadRequest("VaultPath and Path required");

    await mgr.MemorizeAsync(req.VaultPath, req.Path, ct);
    return Results.Ok(new { status = "ok" });
});

app.MapPost("/reindex", (ReindexRequest req) =>
{
    if (string.IsNullOrWhiteSpace(req.VaultPath))
        return Results.BadRequest("VaultPath required");

    _ = Task.Run(async () =>
    {
        try { await mgr.ReindexAsync(req.VaultPath, CancellationToken.None); }
        catch (Exception ex)
        {
            reindexLog.LogError(ex, "Reindex failed for {Vault}", req.VaultPath);
            app.Services.GetRequiredService<ITelemetryEmitter>()
                .ReportError(TelemetryEventName.EmbedderInitFailed, "embedder", ModelPhase.Error, ex);
        }
    });
    return Results.Accepted();
});

app.MapPost("/search", async (SearchRequest req, CancellationToken ct) =>
{
    // Empty Query is allowed (empty results are harmless); empty Vault/Scope mis-scopes silently.
    if (string.IsNullOrWhiteSpace(req.VaultPath) || string.IsNullOrWhiteSpace(req.ScopePath))
        return Results.BadRequest("VaultPath and ScopePath required");

    if (!mgr.EmbedderReady)
        return Results.Ok(new SearchResponse([], "warming"));

    var hits = await mgr.SearchAsync(req.VaultPath, req.Query, req.ScopePath, req.Limit, ct);
    var results = hits
        .Select(h => new SearchHitDto(h.SourcePath, h.Snippet, h.Score))
        .ToList();
    return Results.Ok(new SearchResponse(results, "ok"));
});

app.MapPost("/chat", async (ChatRequestDto req, ITextGenerator gen, HttpContext ctx, CancellationToken ct) =>
{
    if (req.Messages is null || req.Messages.Count == 0)
        return Results.BadRequest("messages required");

    // Lazy-load safety net: if the model has not been prepared yet (no prior
    // /prepare-generation call), load it now before we begin streaming.
    if (!gen.Ready)
        await gen.PrepareAsync(ct);

    ctx.Response.ContentType = "text/event-stream";
    var msgs = req.Messages.Select(m => new ChatMessage(m.Role, m.Content)).ToList();
    var genOpts = new GenerationOptions(MaxTokens: req.MaxTokens ?? 512);

    // ct = HttpContext.RequestAborted -> client disconnect stops generation, frees CPU.
    await foreach (var chunk in gen.GenerateAsync(msgs, genOpts, ct))
    {
        var json = JsonSerializer.Serialize(new { choices = new[] { new { delta = new { content = chunk } } } });
        await ctx.Response.WriteAsync($"data: {json}\n\n", ct);
        await ctx.Response.Body.FlushAsync(ct);
    }
    await ctx.Response.WriteAsync("data: [DONE]\n\n", ct);
    // The handler streams directly into ctx.Response (SSE); there is no object to return.
    // Results.Empty is a formality required by the IResult-returning delegate signature — it
    // writes nothing extra to the response. On client disconnect the OperationCanceledException
    // propagates before this line is reached, so it is unreachable on the abort path.
    return Results.Empty;
});

app.MapPost("/shutdown", (IHostApplicationLifetime life) =>
{
    life.StopApplication();
    return Results.Accepted();
});

app.Run();

// Exposes the implicit Program class for WebApplicationFactory<Program> in tests
// (top-level statements compile to an internal Program; the partial makes it addressable).
public partial class Program { }
