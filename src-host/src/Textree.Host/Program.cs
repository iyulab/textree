using System.Text.Json;
using LMSupply.Embedder;
using Textree.Host;
using Textree.Host.Contracts;
using Textree.Host.Rag;

// ── Startup: load embedder synchronously before the app begins listening ──────
// Plan B's Rust lifecycle polls /health and tolerates the host being slow to
// respond on first-run model download (connection-refused → retry). No
// background-load mechanism needed (YAGNI).
var opts = new TextreeHostOptions();
// Force CPU execution provider: DirectML (the default Auto provider) crashes during
// inference on this hardware ("LayerNormalization DmlExecutionProvider 0x80070057")
// and does not fall back to CPU automatically. e5-small is fast enough on CPU for
// the latencies Textree needs, so CPU is the robust production choice here.
var model = await LocalEmbedder.LoadAsync(
    opts.EmbeddingModel,
    new EmbedderOptions { Provider = LMSupply.ExecutionProvider.Cpu });
var embedder = new LmSupplyEmbeddingService(model);

// ── DI / builder ──────────────────────────────────────────────────────────────
var builder = WebApplication.CreateBuilder(args);
builder.Services.AddSingleton(opts);
builder.Services.AddSingleton(embedder);
builder.Services.AddSingleton<VaultManager>();
// Local CPU text generator for /chat. Singleton: the model loads once (lazily, on first
// /chat) and is reused. Tests replace this with a stub via ConfigureTestServices.
builder.Services.AddSingleton<ITextGenerator, LocalTextGenerator>();

var app = builder.Build();

var mgr = app.Services.GetRequiredService<VaultManager>();
var reindexLog = app.Services.GetRequiredService<ILoggerFactory>().CreateLogger("Textree.Host.Reindex");
var genLog = app.Services.GetRequiredService<ILoggerFactory>().CreateLogger("Textree.Host.Generation");

// ── Endpoints ─────────────────────────────────────────────────────────────────

app.MapGet("/health", (ITextGenerator gen) =>
    Results.Ok(new { status = "ok", embedderReady = mgr.EmbedderReady, generatorReady = gen.Ready, generatorError = gen.LastError }));

app.MapPost("/prepare-generation", (ITextGenerator gen) =>
{
    // Idempotent: kick off model load in the background, return immediately.
    // PrepareAsync is internally idempotent (double-checked SemaphoreSlim), so
    // concurrent or repeated calls are safe.
    _ = Task.Run(async () =>
    {
        try { await gen.PrepareAsync(CancellationToken.None); }
        catch (Exception ex) { genLog.LogError(ex, "Generation model PrepareAsync failed"); }
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
        catch (Exception ex) { reindexLog.LogError(ex, "Reindex failed for {Vault}", req.VaultPath); }
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
    var opts = new GenerationOptions(MaxTokens: req.MaxTokens ?? 512);

    // ct = HttpContext.RequestAborted -> client disconnect stops generation, frees CPU.
    await foreach (var chunk in gen.GenerateAsync(msgs, opts, ct))
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
