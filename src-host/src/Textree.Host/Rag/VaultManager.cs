using FileFlux;
using FluxIndex.Core.Application.Interfaces;
using FluxIndex.Extensions.FileVault.Extensions;
using FluxIndex.Extensions.FileVault.Interfaces;
using FluxIndex.Storage.SQLite;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using IEmbeddingService = FluxIndex.Core.Application.Interfaces.IEmbeddingService;

namespace Textree.Host.Rag;

/// <summary>
/// Owns the single active vault. The embedder is shared (loaded once); each distinct
/// vault path gets its own vector store DB (keyed by <see cref="VaultHash"/>) under
/// <c>IndexRoot/&lt;hash&gt;.db</c>. Switching vaults rebuilds the provider for the new path.
/// </summary>
public sealed class VaultManager : IDisposable
{
    private readonly LmSupplyEmbeddingService _embedder;
    private readonly TextreeHostOptions _options;
    private readonly Lock _gate = new();

    private string? _currentVaultPath;
    private ServiceProvider? _provider;
    private IServiceScope? _scope;
    private IVault? _vault;

    public VaultManager(LmSupplyEmbeddingService embedder, TextreeHostOptions options)
    {
        _embedder = embedder ?? throw new ArgumentNullException(nameof(embedder));
        _options = options ?? throw new ArgumentNullException(nameof(options));
    }

    public bool EmbedderReady => _embedder.Dimensions > 0;

    /// <summary>
    /// Ensures the active vault targets <paramref name="vaultPath"/>, (re)building the
    /// FluxIndex provider if the path changed. Returns the resolved vault.
    /// </summary>
    public IVault EnsureVault(string vaultPath)
    {
        var fullPath = Path.GetFullPath(vaultPath);
        lock (_gate)
        {
            if (_vault is not null &&
                string.Equals(_currentVaultPath, fullPath, StringComparison.OrdinalIgnoreCase))
            {
                return _vault;
            }

            // Tear down the previous vault and clear the fields BEFORE building the new one.
            // If the DI build below throws, the manager must not be left holding a disposed
            // provider/scope, nor a _currentVaultPath that would early-return a dead vault on
            // a same-path retry. New values are assigned only after a fully successful build.
            _scope?.Dispose();
            _provider?.Dispose();
            _scope = null;
            _provider = null;
            _vault = null;
            _currentVaultPath = null;

            // VaultHash.For owns path canonicalization (it re-runs Path.GetFullPath + casing/
            // separator normalization internally); fullPath here is already normalized purely
            // for the cache-key comparison above. The repeated GetFullPath is harmless.
            var hash = VaultHash.For(fullPath);
            Directory.CreateDirectory(_options.IndexRoot);
            var dbPath = Path.Combine(_options.IndexRoot, $"{hash}.db");
            // FileVault's VaultBasePath is its STORAGE root (extracted/refined .md, per-entry
            // dirs), NOT the content tree. Keep it under IndexRoot — distinct from the user's
            // notes — so the store never pollutes the content folder. Memorized files are
            // referenced by absolute path, so scope filtering still targets the real note paths.
            var vaultStore = Path.Combine(_options.IndexRoot, hash);

            // Identity binds the vectors to the embedding model. The fingerprint must be set
            // on the SQLite options so the vec table name is deterministic at migration time,
            // and BindIdentity must run on the resolved vector store before any store op
            // (FluxIndex 0.13.x — same contract Filer satisfies via VectorStoreIdentityBinder).
            var identity = _embedder.GetIdentity();

            var services = new ServiceCollection();
            services.AddLogging();

            // Shared embedder (singleton): determines the vector store dimension.
            services.TryAddSingleton<IEmbeddingService>(_embedder);

            services.AddSQLiteVecVectorStore(o =>
            {
                o.DatabasePath = dbPath;
                o.VectorDimension = _embedder.Dimensions;
                o.UseSQLiteVec = true;
                o.AutoMigrate = true;
                o.EmbeddingFingerprint = identity.Fingerprint;
            });

            services.AddFileFlux();

            services.AddFileVaultWithFluxIndex(o =>
            {
                o.VaultBasePath = vaultStore;
                // No IHostedService host here (bare ServiceProvider): run the memorize
                // pipeline synchronously inside MemorizeAsync rather than via a background queue.
                o.EnableBackgroundProcessing = false;
                o.Chunking.Strategy = "Hierarchical";
                o.Chunking.MaxChunkSize = 1024;
                o.Chunking.OverlapSize = 128;
            });

            var provider = services.BuildServiceProvider();

            // Run registered hosted services (e.g. the SQLite vec-table migration) — a bare
            // ServiceProvider has no Host to start them, so the vec table would never be created.
            foreach (var hosted in provider.GetServices<Microsoft.Extensions.Hosting.IHostedService>())
            {
                hosted.StartAsync(CancellationToken.None).GetAwaiter().GetResult();
            }

            var scope = provider.CreateScope();

            // Bind the embedding identity to the vector store before any vault op (vec table naming).
            scope.ServiceProvider.GetRequiredService<IVectorStore>().BindIdentity(identity);

            var vault = scope.ServiceProvider.GetRequiredService<IVault>();

            _provider = provider;
            _scope = scope;
            _vault = vault;
            _currentVaultPath = fullPath;
            return vault;
        }
    }

    public async Task MemorizeAsync(string vaultPath, string filePath, CancellationToken ct = default)
    {
        var vault = EnsureVault(vaultPath);
        var fullPath = Path.GetFullPath(filePath);

        // Upsert semantics: if an entry already exists, remove it first so that re-memorizing
        // the same file does not accumulate duplicate chunks in the vector store. Without this,
        // repeated ReindexAsync calls grow the result set linearly with the reindex count.
        // Sequential by design: Rust forwards file events one-at-a-time over a single IPC channel,
        // so there is no concurrent MemorizeAsync for the same path. A future parallel reindex
        // would need a per-path lock (or a native upsert API) to keep this get→remove→add atomic.
        var existing = await vault.GetAsync(fullPath, ct);
        if (existing is not null)
        {
            await vault.RemoveAsync(fullPath, ct);
        }

        await vault.MemorizeAsync(fullPath, ct);

        // With EnableBackgroundProcessing=false the pipeline runs inline, so the returned entry
        // is normally already terminal. Poll defensively until a terminal stage, then surface
        // failures loudly — a silent Error/timeout would let ReindexAsync mark a vault "indexed"
        // while chunks are missing (project rule: never swallow failures without signal).
        var deadline = Environment.TickCount64 + 60_000;
        while (Environment.TickCount64 < deadline)
        {
            var entry = await vault.GetAsync(fullPath, ct);
            if (entry is not null)
            {
                if (entry.Stage is FluxIndex.Extensions.FileVault.Domain.Enums.ProcessingStage.Memorized)
                {
                    return;
                }
                if (entry.Stage is FluxIndex.Extensions.FileVault.Domain.Enums.ProcessingStage.Error)
                {
                    throw new InvalidOperationException(
                        $"FluxIndex pipeline error for '{fullPath}': {entry.LastError}");
                }
            }
            await Task.Delay(100, ct);
        }

        throw new TimeoutException($"Memorize timed out for '{fullPath}'");
    }

    public async Task<IReadOnlyList<SearchHit>> SearchAsync(
        string vaultPath, string query, string scopePath, int limit, CancellationToken ct = default)
    {
        var vault = EnsureVault(vaultPath);
        var options = new VaultSearchOptions
        {
            TopK = limit,
            MinScore = 0,
            PathScope = [Path.GetFullPath(scopePath)],
            IncludeContent = true,
            IncludeMetadata = true,
        };

        var result = await vault.SearchAsync(query, options, ct);
        return result.Items
            .Select(i => new SearchHit(
                i.SourcePath,
                Snippet(i.Content),
                i.Score))
            .ToList();
    }

    public async Task ReindexAsync(string vaultPath, CancellationToken ct = default)
    {
        var fullPath = Path.GetFullPath(vaultPath);
        EnsureVault(fullPath);
        foreach (var file in Directory.EnumerateFiles(fullPath, "*.md", SearchOption.AllDirectories))
        {
            ct.ThrowIfCancellationRequested();
            await MemorizeAsync(fullPath, file, ct);
        }
    }

    private static string Snippet(string? content)
    {
        if (string.IsNullOrEmpty(content)) return string.Empty;
        return content.Length <= 200 ? content : content[..200];
    }

    /// <summary>
    /// Disposes the active vault's scope and provider (SQLite connections, file handles).
    /// The shared embedder is owned by the caller — never disposed here.
    /// </summary>
    public void Dispose()
    {
        lock (_gate)
        {
            _scope?.Dispose();
            _provider?.Dispose();
            _scope = null;
            _provider = null;
            _vault = null;
            _currentVaultPath = null;
        }
    }
}

public readonly record struct SearchHit(string SourcePath, string Snippet, float Score);
