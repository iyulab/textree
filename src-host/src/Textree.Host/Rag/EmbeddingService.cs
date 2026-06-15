using FluxIndex.Core.Application.Services.Base;
using LMSupply.Embedder;

namespace Textree.Host.Rag;

/// <summary>
/// Adapts LMSupply's <see cref="IEmbeddingModel"/> to FluxIndex's
/// <c>FluxIndex.Core.Application.Interfaces.IEmbeddingService</c> via the
/// <see cref="EmbeddingServiceBase"/> template (same pattern as Filer's
/// LMSupplyEmbeddingService). The base class supplies <c>IEmbeddingService</c>;
/// we override the model-backed members.
/// </summary>
public sealed class LmSupplyEmbeddingService : EmbeddingServiceBase
{
    private readonly IEmbeddingModel _model;

    public LmSupplyEmbeddingService(IEmbeddingModel model)
    {
        ArgumentNullException.ThrowIfNull(model);
        _model = model;
    }

    /// <summary>Intrinsic embedding dimension of the loaded model (e.g. 384 for e5-small).</summary>
    public int Dimensions => _model.Dimensions;

    protected override async Task<float[]> EmbedCoreAsync(string text, CancellationToken cancellationToken)
        => await _model.EmbedAsync(text, cancellationToken);

    public override async Task<IEnumerable<float[]>> GenerateEmbeddingsBatchAsync(
        IEnumerable<string> texts, CancellationToken cancellationToken = default)
        => await _model.EmbedAsync(texts.ToList(), cancellationToken);

    public override int GetEmbeddingDimension() => _model.Dimensions;

    public override string GetModelName() => _model.ModelId;

    protected override string GetProviderName() => "local";
}
