using LMSupply;

namespace Textree.Host.Rag;

public enum ModelPhase { Idle, Downloading, Loading, Ready, Error }

public record ModelSnapshot(
    ModelPhase Phase, double OverallPercent, long BytesDownloaded,
    long TotalBytes, int FileIndex, int FileCount, string? Error)
{
    public static readonly ModelSnapshot Idle =
        new(ModelPhase.Idle, 0, 0, 0, 0, 0, null);
}

/// <summary>
/// Thread-safe shared model download/readiness state. The embedder and generator each report
/// download progress here via an <see cref="IProgress{T}"/> adapter; /health reads the snapshots.
/// Snapshots are immutable records swapped via Volatile, so readers never see a torn value.
/// </summary>
public sealed class ModelStatus
{
    private ModelSnapshot _embedder = ModelSnapshot.Idle;
    private ModelSnapshot _generator = ModelSnapshot.Idle;

    public ModelSnapshot Embedder => Volatile.Read(ref _embedder);
    public ModelSnapshot Generator => Volatile.Read(ref _generator);

    public IProgress<DownloadProgress> EmbedderProgress { get; }
    public IProgress<DownloadProgress> GeneratorProgress { get; }

    public ModelStatus()
    {
        EmbedderProgress = new SyncProgress(p => Report(ref _embedder, p));
        GeneratorProgress = new SyncProgress(p => Report(ref _generator, p));
    }

    private static void Report(ref ModelSnapshot slot, DownloadProgress p) =>
        Volatile.Write(ref slot, new ModelSnapshot(
            ModelPhase.Downloading, p.OverallPercentComplete, p.BytesDownloaded,
            p.TotalBytes, p.CurrentFileIndex, p.TotalFileCount, null));

    public void SetEmbedderPhase(ModelPhase phase) => SetPhase(ref _embedder, phase);
    public void SetGeneratorPhase(ModelPhase phase) => SetPhase(ref _generator, phase);
    public void SetEmbedderError(string msg) => SetError(ref _embedder, msg);
    public void SetGeneratorError(string msg) => SetError(ref _generator, msg);

    private static void SetPhase(ref ModelSnapshot slot, ModelPhase phase)
    {
        var cur = Volatile.Read(ref slot);
        Volatile.Write(ref slot, cur with { Phase = phase, Error = phase == ModelPhase.Error ? cur.Error : null });
    }

    private static void SetError(ref ModelSnapshot slot, string msg)
    {
        var cur = Volatile.Read(ref slot);
        Volatile.Write(ref slot, cur with { Phase = ModelPhase.Error, Error = msg });
    }

    /// <summary>
    /// Synchronous <see cref="IProgress{T}"/> adapter that invokes the callback on the calling
    /// thread, guaranteeing test determinism and avoiding any captured
    /// <see cref="System.Threading.SynchronizationContext"/> scheduling.
    /// </summary>
    private sealed class SyncProgress(Action<DownloadProgress> on) : IProgress<DownloadProgress>
    {
        public void Report(DownloadProgress value) => on(value);
    }
}
