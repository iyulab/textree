using LMSupply;
using LMSupply.Embedder;

namespace Textree.Host.Rag;

/// <summary>
/// Abstracts the blocking model-load call so tests can inject a stub loader without
/// actually downloading or loading the ONNX model at startup.
/// </summary>
public interface IEmbedderLoader
{
    /// <summary>
    /// Loads the embedding model identified by <paramref name="modelId"/>, reporting
    /// download progress via <paramref name="progress"/> as bytes arrive.
    /// </summary>
    Task<IEmbeddingModel> LoadAsync(
        string modelId,
        IProgress<DownloadProgress> progress,
        CancellationToken ct);
}

/// <summary>
/// Production implementation: delegates directly to <see cref="LocalEmbedder.LoadAsync"/>.
/// Forces the CPU execution provider because DirectML crashes on the target hardware during
/// inference and does not fall back automatically (see Program.cs note on ExecutionProvider.Cpu).
/// </summary>
public sealed class DefaultEmbedderLoader : IEmbedderLoader
{
    public Task<IEmbeddingModel> LoadAsync(
        string modelId,
        IProgress<DownloadProgress> progress,
        CancellationToken ct)
        => LocalEmbedder.LoadAsync(
            modelId,
            new EmbedderOptions { Provider = ExecutionProvider.Cpu },
            progress,
            ct);
}
