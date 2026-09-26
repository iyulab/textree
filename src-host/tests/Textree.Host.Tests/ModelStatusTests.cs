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
        // Second file of three: 0.4 GB already done before it, 0.8 GB of this 2.5 GB file.
        var progress = new DownloadProgress
        {
            FileName = "model.onnx.data",
            BytesDownloaded = 800_000_000,
            TotalBytes = 2_500_000_000,
            OverallBytesDownloaded = 1_200_000_000,
            OverallTotalBytes = 2_900_000_000,
            CurrentFileIndex = 2,
            TotalFileCount = 3,
        };
        status.EmbedderProgress.Report(progress);

        var snap = status.Embedder;
        Assert.Equal(ModelPhase.Downloading, snap.Phase);
        // Assert exact copy of the computed OverallPercentComplete — verifies Report reads the right property.
        Assert.Equal(progress.OverallPercentComplete, snap.OverallPercent);
        // The byte pair is the whole download, not the current file.
        Assert.Equal(1_200_000_000, snap.BytesDownloaded);
        Assert.Equal(2_900_000_000, snap.TotalBytes);
        Assert.Equal(2, snap.FileIndex);
        Assert.Equal(3, snap.FileCount);
        Assert.Null(snap.Error);
    }

    [Fact]
    public void Single_file_download_reports_that_files_bytes()
    {
        var status = new ModelStatus();
        status.GeneratorProgress.Report(new DownloadProgress
        {
            FileName = "model.gguf",
            BytesDownloaded = 300,
            TotalBytes = 1_000,
        });

        Assert.Equal(300, status.Generator.BytesDownloaded);
        Assert.Equal(1_000, status.Generator.TotalBytes);
    }

    [Fact]
    public void Multi_file_download_with_unknown_sizes_reports_no_bytes()
    {
        // Per-file bytes must not pass for the whole download: the "X / Y GB" detail would show
        // one file's size as if it were everything.
        var status = new ModelStatus();
        var progress = new DownloadProgress
        {
            FileName = "b.bin",
            BytesDownloaded = 300,
            TotalBytes = 1_000,
            CurrentFileIndex = 2,
            TotalFileCount = 4,
        };
        status.GeneratorProgress.Report(progress);

        Assert.Equal(0, status.Generator.BytesDownloaded);
        Assert.Equal(0, status.Generator.TotalBytes);
        Assert.Equal(progress.OverallPercentComplete, status.Generator.OverallPercent);
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
