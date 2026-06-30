using Microsoft.ApplicationInsights.Channel;
using Microsoft.ApplicationInsights.Extensibility;

namespace Textree.Host.Telemetry;

/// <summary>
/// Clears identifying context from telemetry that survives the allowlist gate. Implemented as a
/// processor (not an ITelemetryInitializer) because App Insights' TelemetryClient.Initialize()
/// re-stamps Cloud.RoleInstance with the machine hostname AFTER initializers run when it is empty;
/// a processor runs later, in the send pipeline, and reliably overwrites all post-init SDK stamping.
/// </summary>
public sealed class ContextScrubProcessor(ITelemetryProcessor next) : ITelemetryProcessor
{
    public void Process(ITelemetry item)
    {
        item.Context.Cloud.RoleInstance = string.Empty;
        item.Context.Cloud.RoleName = string.Empty;
        item.Context.Device.Id = string.Empty;
        item.Context.User.Id = string.Empty;
        item.Context.User.AuthenticatedUserId = string.Empty;
        item.Context.Location.Ip = string.Empty;
        next.Process(item);
    }
}
