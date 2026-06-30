namespace Textree.Host.Telemetry;

/// <summary>
/// On/off gate for outbound telemetry. The connection string is the only switch:
/// absent or blank ⇒ disabled. This single rule covers dev/test (no string set),
/// offline-by-config, and MIT forks of the official build (which ship no string).
/// </summary>
public sealed record TelemetryOptions(bool IsEnabled, string? ConnectionString)
{
    public const string EnvVar = "TEXTREE_TELEMETRY_CONNECTION";

    public static TelemetryOptions Read(IDictionary<string, string?> env)
    {
        env.TryGetValue(EnvVar, out var raw);
        return string.IsNullOrWhiteSpace(raw)
            ? new TelemetryOptions(false, null)
            : new TelemetryOptions(true, raw);
    }
}
