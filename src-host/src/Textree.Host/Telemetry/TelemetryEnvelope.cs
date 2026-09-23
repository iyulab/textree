using System.Text.Json.Nodes;

namespace Textree.Host.Telemetry;

/// <summary>
/// The one shape that leaves the host: an Application Insights <c>EventData</c> envelope, written
/// field by field. Nothing is added by a library — no machine name, device, user, session or IP
/// context exists unless a line here writes it, and none does. The public ingestion schema is the
/// contract (the same one the app's own egress writes).
/// </summary>
public static class TelemetryEnvelope
{
    public const string CloudRole = "textree-host";

    public static JsonObject Build(
        TelemetryConnection connection, string eventName,
        IReadOnlyDictionary<string, string> properties, string appVersion, DateTimeOffset time)
    {
        var props = new JsonObject();
        foreach (var (k, v) in properties) props[k] = v;
        return new JsonObject
        {
            ["name"] = "Microsoft.ApplicationInsights.Event",
            ["time"] = time.UtcDateTime.ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'"),
            ["iKey"] = connection.InstrumentationKey,
            ["tags"] = new JsonObject
            {
                ["ai.cloud.role"] = CloudRole,
                ["ai.application.ver"] = appVersion,
            },
            ["data"] = new JsonObject
            {
                ["baseType"] = "EventData",
                ["baseData"] = new JsonObject
                {
                    ["ver"] = 2,
                    ["name"] = eventName,
                    ["properties"] = props,
                },
            },
        };
    }
}
