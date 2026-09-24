using FileFlux;
using FluxIndex.Core.Application.Interfaces;
using FluxFeed.Domain.Enums;
using FluxFeed.Extensions;
using FluxFeed.Interfaces;
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
    private readonly EmbedderSlot _embedder;
    private readonly TextreeHostOptions _options;
    private readonly Lock _gate = new();

    private string? _currentVaultPath;
    private ServiceProvider? _provider;
    private IServiceScope? _scope;
    private IVault? _vault;

    public VaultManager(EmbedderSlot embedder, TextreeHostOptions options)
    {
        _embedder = embedder ?? throw new ArgumentNullException(nameof(embedder));
        _options = options ?? throw new ArgumentNullException(nameof(options));
    }

    public bool EmbedderReady => _embedder.Ready;

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
            // FluxFeed's VaultBasePath is its STORAGE root (extracted/refined .md, per-entry
            // dirs), NOT the content tree. Keep it under IndexRoot — distinct from the user's
            // notes — so the store never pollutes the content folder. Memorized files are
            // referenced by absolute path, so scope filtering still targets the real note paths.
            var vaultStore = Path.Combine(_options.IndexRoot, hash);

            // Identity binds the vectors to the embedding model. The fingerprint is set on the
            // SQLite options so the vec table name is deterministic; FluxFeed binds the store to
            // the embedder's identity itself and refuses a store bound to a different embedder.
            var embedding = _embedder.Service;
            var identity = embedding.GetIdentity();

            var services = new ServiceCollection();
            services.AddLogging();

            // Shared embedder (singleton): determines the vector store dimension.
            services.TryAddSingleton<IEmbeddingService>(embedding);

            services.AddSQLiteVecVectorStore(o =>
            {
                o.DatabasePath = dbPath;
                o.VectorDimension = embedding.GetEmbeddingDimension();
                o.UseSQLiteVec = true;
                o.AutoMigrate = true;
                o.EmbeddingFingerprint = identity.Fingerprint;
            });

            services.AddFileFlux();

            services.AddFileVaultWithFluxIndex(o =>
            {
                o.VaultBasePath = vaultStore;
                // No Generic Host here (bare ServiceProvider): the memorize pipeline runs inline
                // inside MemorizeAsync, so there is no background worker that would need starting.
                o.EnableBackgroundProcessing = false;
                o.Chunking.Strategy = "Hierarchical";
                o.Chunking.MaxChunkSize = 1024;
                o.Chunking.OverlapSize = 128;
            });

            var provider = services.BuildServiceProvider();
            var scope = provider.CreateScope();
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

        // Re-memorizing replaces the file's previous chunks (chunk ids derive from the path and
        // the passage), so an edited note needs no remove first and an unchanged one does not
        // accumulate duplicates. With background processing off the pipeline runs inline and the
        // returned entry is terminal — but a failure there comes back as an Error entry rather
        // than an exception, and a silent Error would let ReindexAsync call a vault "indexed"
        // while chunks are missing (project rule: never swallow failures without signal).
        var entry = await vault.MemorizeAsync(fullPath, waitForCompletion: true, ct);
        if (entry.Stage is ProcessingStage.Error)
        {
            throw new InvalidOperationException(
                $"Indexing failed for '{fullPath}': {entry.FirstError ?? entry.LastError}");
        }
    }

    public async Task<IReadOnlyList<SearchHit>> SearchAsync(
        string vaultPath, string query, string scopePath, int limit, CancellationToken ct = default)
    {
        var fullVaultPath = Path.GetFullPath(vaultPath);
        var vault = EnsureVault(fullVaultPath);
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
                // Return vault-relative POSIX path (e.g. "notes/x.md") rather than the
                // absolute Windows path stored in the vector index. The Rust/frontend layers
                // expect vault-relative POSIX paths: absolute paths break self-exclusion in
                // the Related notes panel and note navigation from semantic search hits.
                Path.GetRelativePath(fullVaultPath, i.SourcePath).Replace('\\', '/'),
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
