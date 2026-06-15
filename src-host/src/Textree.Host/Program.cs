using LMSupply.Embedder;
using Textree.Host;
using Textree.Host.Contracts;
using Textree.Host.Rag;

// ── Startup: load embedder synchronously before the app begins listening ──────
// Plan B's Rust lifecycle polls /health and tolerates the host being slow to
// respond on first-run model download (connection-refused → retry). No
// background-load mechanism needed (YAGNI).
var opts = new TextreeHostOptions();
var model = await LocalEmbedder.LoadAsync(opts.EmbeddingModel);
var embedder = new LmSupplyEmbeddingService(model);

// ── DI / builder ──────────────────────────────────────────────────────────────
var builder = WebApplication.CreateBuilder(args);
builder.Services.AddSingleton(opts);
builder.Services.AddSingleton(embedder);
builder.Services.AddSingleton<VaultManager>();

var app = builder.Build();

var mgr = app.Services.GetRequiredService<VaultManager>();
var reindexLog = app.Services.GetRequiredService<ILoggerFactory>().CreateLogger("Textree.Host.Reindex");

// ── Endpoints ─────────────────────────────────────────────────────────────────

app.MapGet("/health", () =>
    Results.Ok(new { status = "ok", embedderReady = mgr.EmbedderReady }));

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

app.MapPost("/shutdown", (IHostApplicationLifetime life) =>
{
    life.StopApplication();
    return Results.Accepted();
});

app.Run();
