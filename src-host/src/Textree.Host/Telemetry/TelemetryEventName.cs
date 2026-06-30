namespace Textree.Host.Telemetry;

/// <summary>Stable, enumerable error event codes — the telemetry allowlist's event dimension.</summary>
public static class TelemetryEventName
{
    public const string ModelDownloadFailed = "model.download.failed";
    public const string ModelLoadFailed = "model.load.failed";
    public const string EmbedderInitFailed = "embedder.init.failed";
    public const string ReindexFailed = "reindex.failed";
    public const string GenerationPrepareFailed = "generation.prepare.failed";
    public const string HostStartupFailed = "host.startup.failed";

    public static readonly IReadOnlySet<string> All = new HashSet<string>
    {
        ModelDownloadFailed, ModelLoadFailed, EmbedderInitFailed, ReindexFailed,
        GenerationPrepareFailed, HostStartupFailed,
    };
}
