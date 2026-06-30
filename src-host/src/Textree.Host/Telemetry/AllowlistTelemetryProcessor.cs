using Microsoft.ApplicationInsights.Channel;
using Microsoft.ApplicationInsights.DataContracts;
using Microsoft.ApplicationInsights.Extensibility;

namespace Textree.Host.Telemetry;

/// <summary>
/// Fail-closed gate. Only EventTelemetry whose Name is on <see cref="TelemetryEventName.All"/> proceeds;
/// EVERYTHING else is dropped — including every auto-collected telemetry type (request/dependency/
/// exception/trace), which can carry URLs, machine names, paths, and full stack traces. The default
/// is "drop", so an unrecognized future telemetry shape also fails safe without a code change.
/// </summary>
public sealed class AllowlistTelemetryProcessor(ITelemetryProcessor next) : ITelemetryProcessor
{
    public void Process(ITelemetry item)
    {
        if (item is EventTelemetry e && TelemetryEventName.All.Contains(e.Name))
            next.Process(item);
        // else: dropped silently — this is the safety default.
    }
}
