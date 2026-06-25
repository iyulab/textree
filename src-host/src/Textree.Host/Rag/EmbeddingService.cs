using FluxIndex.Core.Application.Services.Base;
using LMSupply.Embedder;

namespace Textree.Host.Rag;

// NOTE: FluxIndex.Providers.LMSupply.LMSupplyEmbeddingService exists but signature diverged at 0.13.19;
// revisit. The published adapter (FluxIndex.Providers.LMSupply.Services.LMSupplyEmbeddingService) does
// not expose a public Dimensions property — only GetEmbeddingDimension() via the interface.
// VaultManager depends on .Dimensions directly (EmbedderReady and SQLite vector dimension), so
// dropping this hand-rolled adapter would require refactoring VaultManager as well. Keeping hand-rolled
// until the published adapter aligns or VaultManager is updated to use GetEmbeddingDimension().

/// <summary>
/// Adapts LMSupply's <see cref="IEmbeddingModel"/> to FluxIndex's
/// <c>FluxIndex.Core.Application.Interfaces.IEmbeddingService</c> via the
/// <see cref="EmbeddingServiceBase"/> template (same pattern as Filer's
/// LMSupplyEmbeddingService). The base class supplies <c>IEmbeddingService</c>;
/// we override the model-backed members.
/// </summary>
/// <remarks>
/// The model is optional at construction time to support lazy / background loading.
/// <see cref="Dimensions"/> returns <c>0</c> until <see cref="SetModel"/> is called,
/// which drives <c>VaultManager.EmbedderReady</c> to remain <see langword="false"/>
/// until the model is fully loaded. All embedding operations throw
/// <see cref="InvalidOperationException"/> when the model has not been set.
/// </remarks>
public sealed class LmSupplyEmbeddingService : EmbeddingServiceBase
{
    private IEmbeddingModel? _model;

    /// <summary>Lazy constructor — model must be provided later via <see cref="SetModel"/>.</summary>
    public LmSupplyEmbeddingService() { }

    /// <summary>Eager constructor for back-compat (existing tests and Program.cs startup path).</summary>
    public LmSupplyEmbeddingService(IEmbeddingModel model)
    {
        ArgumentNullException.ThrowIfNull(model);
        _model = model;
    }

    /// <summary>Sets the embedding model after construction (lazy / background-load path).</summary>
    public void SetModel(IEmbeddingModel model)
        => _model = model ?? throw new ArgumentNullException(nameof(model));

    /// <summary>
    /// Intrinsic embedding dimension of the loaded model (e.g. 384 for e5-small).
    /// Returns <c>0</c> until the model is loaded — drives <c>VaultManager.EmbedderReady</c>
    /// naturally false until ready.
    /// </summary>
    public int Dimensions => _model?.Dimensions ?? 0;

    // Private guard: all embed paths route through this to get a clean, uniform error.
    private IEmbeddingModel Model => _model
        ?? throw new InvalidOperationException("Embedder model not loaded yet.");

    protected override async Task<float[]> EmbedCoreAsync(string text, CancellationToken cancellationToken)
        => await Model.EmbedAsync(text, cancellationToken);

    public override async Task<IEnumerable<float[]>> GenerateEmbeddingsBatchAsync(
        IEnumerable<string> texts, CancellationToken cancellationToken = default)
        => await Model.EmbedAsync(texts.ToList(), cancellationToken);

    public override int GetEmbeddingDimension() => _model?.Dimensions ?? 0;

    public override string GetModelName() => _model?.ModelId ?? "loading";

    protected override string GetProviderName() => "local";
}
