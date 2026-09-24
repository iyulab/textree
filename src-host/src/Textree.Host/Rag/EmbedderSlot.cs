using FluxIndex.Providers.LMSupply.Services;
using LMSupply.Embedder;

namespace Textree.Host.Rag;

/// <summary>
/// Where the embedding service appears once its model has loaded.
/// </summary>
/// <remarks>
/// The host starts answering before the model is on disk: the model downloads and loads in the
/// background while <c>/health</c> reports progress. Until then there is no service, and anything
/// that needs one asks <see cref="Ready"/> first. The service itself is FluxIndex's own LMSupply
/// adapter — this type only holds the moment it becomes available.
/// </remarks>
public sealed class EmbedderSlot
{
    private volatile LMSupplyEmbeddingService? _service;
    private volatile IEmbeddingModel? _model;

    /// <summary>Whether the model has loaded and the service exists.</summary>
    public bool Ready => _service is not null;

    /// <summary>The service. Throws while the model is still loading.</summary>
    public LMSupplyEmbeddingService Service =>
        _service ?? throw new InvalidOperationException("Embedder model not loaded yet.");

    /// <summary>Makes the service available, wrapping the loaded <paramref name="model"/>.</summary>
    public void Set(IEmbeddingModel model)
    {
        ArgumentNullException.ThrowIfNull(model);
        _model = model;
        _service = new LMSupplyEmbeddingService(model);
    }

    /// <summary>What the loaded model actually runs on, as the runtime reports it; null until loaded.</summary>
    public IReadOnlyList<string>? ActiveProviders => _model?.ActiveProviders?.Select(p => p.ToString()).ToList();
}
