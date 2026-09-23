namespace Textree.Host.Telemetry;

/// <summary>
/// Where events go, parsed from an Application Insights connection string
/// (<c>Key=Value;Key=Value;…</c>). Both <c>InstrumentationKey</c> and <c>IngestionEndpoint</c> must be
/// present and non-empty; anything less disables telemetry rather than sending somewhere unintended.
/// Same rule as the app's own egress, so one embedded string turns both on or neither.
/// </summary>
public sealed record TelemetryConnection(string InstrumentationKey, Uri TrackUrl)
{
    public static TelemetryConnection? Parse(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw)) return null;
        string? key = null, endpoint = null;
        foreach (var part in raw.Split(';'))
        {
            var eq = part.IndexOf('=');
            if (eq <= 0) continue;
            var value = part[(eq + 1)..].Trim();
            if (value.Length == 0) continue;
            switch (part[..eq].Trim())
            {
                case "InstrumentationKey": key = value; break;
                case "IngestionEndpoint": endpoint = value; break;
            }
        }
        if (key is null || endpoint is null) return null;
        return Uri.TryCreate($"{endpoint.TrimEnd('/')}/v2/track", UriKind.Absolute, out var url)
            && (url.Scheme == Uri.UriSchemeHttps || url.Scheme == Uri.UriSchemeHttp)
            ? new TelemetryConnection(key, url)
            : null;
    }
}
