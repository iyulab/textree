using LMSupply;
using Textree.Host.Rag;
using Xunit;

public class ModelStatusTests
{
    [Fact]
    public void EmbedderProgress_callback_updates_snapshot_to_downloading()
    {
        var status = new ModelStatus();
        Assert.Equal(ModelPhase.Idle, status.Embedder.Phase);

        status.SetEmbedderPhase(ModelPhase.Downloading);
        // Note: OverallPercentComplete and PercentComplete are computed (read-only) properties.
        // They derive from BytesDownloaded/TotalBytes with file-count weighting.
        var progress = new DownloadProgress
        {
            FileName = "model.onnx.data",
            BytesDownloaded = 1_200_000_000,
            TotalBytes = 2_900_000_000,
            CurrentFileIndex = 2,
            TotalFileCount = 3,
        };
        status.EmbedderProgress.Report(progress);

        var snap = status.Embedder;
        Assert.Equal(ModelPhase.Downloading, snap.Phase);
        // Assert exact copy of the computed OverallPercentComplete — verifies Report reads the right property.
        Assert.Equal(progress.OverallPercentComplete, snap.OverallPercent);
        Assert.Equal(1_200_000_000, snap.BytesDownloaded);
        Assert.Equal(2_900_000_000, snap.TotalBytes);
        Assert.Equal(2, snap.FileIndex);
        Assert.Equal(3, snap.FileCount);
        Assert.Null(snap.Error);
    }

    [Fact]
    public void SetEmbedderError_moves_phase_to_error_with_message()
    {
        var status = new ModelStatus();
        status.SetEmbedderError("download failed");
        Assert.Equal(ModelPhase.Error, status.Embedder.Phase);
        Assert.Equal("download failed", status.Embedder.Error);
    }

    [Fact]
    public void Generator_and_embedder_snapshots_are_independent()
    {
        var status = new ModelStatus();
        status.SetGeneratorPhase(ModelPhase.Ready);
        Assert.Equal(ModelPhase.Ready, status.Generator.Phase);
        Assert.Equal(ModelPhase.Idle, status.Embedder.Phase);
    }

    [Fact]
    public void SetEmbedderPhase_Error_throws_ArgumentException()
    {
        var status = new ModelStatus();
        Assert.Throws<ArgumentException>(() => status.SetEmbedderPhase(ModelPhase.Error));
        // The correct path to Error phase is SetEmbedderError (covered by SetEmbedderError_moves_phase_to_error_with_message).
    }
}
