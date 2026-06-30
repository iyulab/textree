using Microsoft.ApplicationInsights.Channel;
using Microsoft.ApplicationInsights.Extensibility;

namespace Textree.Host.Telemetry;

/// <summary>
/// Second-line defense: unconditionally clears identifying context the SDK may otherwise attach.
/// RoleInstance often carries the machine hostname (and username); device/user ids and client IP
/// are all forbidden PII. Runs on every telemetry item that survives the allowlist processor.
/// </summary>
public sealed class ScrubInitializer : ITelemetryInitializer
{
    public void Initialize(ITelemetry telemetry)
    {
        telemetry.Context.Cloud.RoleInstance = string.Empty;
        telemetry.Context.Cloud.RoleName = string.Empty;
        telemetry.Context.Device.Id = string.Empty;
        telemetry.Context.User.Id = string.Empty;
        telemetry.Context.User.AuthenticatedUserId = string.Empty;
        telemetry.Context.Location.Ip = string.Empty;
    }
}
