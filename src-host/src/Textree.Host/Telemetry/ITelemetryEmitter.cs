using Textree.Host.Rag;

namespace Textree.Host.Telemetry;

/// <summary>Host-facing surface for reporting a content-free error signal. Implementations must never block.</summary>
public interface ITelemetryEmitter
{
    void ReportError(string eventName, string? modelSlot, ModelPhase phase, Exception ex);
}

/// <summary>No-op used when telemetry is disabled (no connection string).</summary>
public sealed class NullTelemetryEmitter : ITelemetryEmitter
{
    public void ReportError(string eventName, string? modelSlot, ModelPhase phase, Exception ex) { }
}
