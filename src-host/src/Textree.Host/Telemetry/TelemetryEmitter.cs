using System.Net.Http.Json;
using System.Text.Json.Nodes;
using Microsoft.Extensions.Logging;
using Textree.Host.Rag;

namespace Textree.Host.Telemetry;

/// <summary>
/// Sends allowlisted error events to the ingestion endpoint, built by <see cref="TelemetryEnvelope"/>.
/// Only the exception TYPE NAME crosses into the payload — never the message or stack. Events not
/// on <see cref="TelemetryEventName.All"/> are dropped. Sends never block the caller and never
/// throw; a send that fails (offline) is dropped — no buffering, no retry. Every send is mirrored to
/// the local logger so the (mandatory, no-opt-out) reporting is visible, not silent.
/// </summary>
public sealed class TelemetryEmitter : ITelemetryEmitter
{
    private static readonly TimeSpan SendTimeout = TimeSpan.FromSeconds(10);

    private readonly TelemetryConnection _connection;
    private readonly HttpClient _http;
    private readonly ILogger _logger;
    private readonly EnvFacts _env;
    private readonly TimeProvider _clock;

    private TelemetryEmitter(
        TelemetryConnection connection, HttpClient http, ILogger logger, EnvFacts env, TimeProvider clock)
    {
        _connection = connection;
        _http = http;
        _logger = logger;
        _env = env;
        _clock = clock;
    }

    /// <param name="handler">Tests capture the real request through this; production uses the default.</param>
    public static ITelemetryEmitter Create(
        TelemetryOptions options, ILogger logger, EnvFacts env,
        HttpMessageHandler? handler = null, TimeProvider? clock = null)
    {
        // Fail-closed: disabled, or a connection string without both key and endpoint.
        var connection = options.IsEnabled ? TelemetryConnection.Parse(options.ConnectionString) : null;
        if (connection is null) return new NullTelemetryEmitter();
        var http = handler is null ? new HttpClient() : new HttpClient(handler);
        http.Timeout = SendTimeout;
        return new TelemetryEmitter(connection, http, logger, env, clock ?? TimeProvider.System);
    }

    public void ReportError(string eventName, string? modelSlot, ModelPhase phase, Exception ex)
    {
        if (!TelemetryEventName.All.Contains(eventName)) return; // the allowlist: anything else is dropped
        var props = TelemetryPayload.BuildErrorProperties(modelSlot, phase, ex.GetType().Name, _env);
        var envelope = TelemetryEnvelope.Build(_connection, eventName, props, _env.AppVersion, _clock.GetUtcNow());
        _logger.LogInformation("telemetry: {Event} sent (slot={Slot}, phase={Phase}, type={Type})",
            eventName, modelSlot ?? "-", phase, ex.GetType().Name);
        _ = SendAsync(new JsonArray(envelope));
    }

    private async Task SendAsync(JsonArray batch)
    {
        try
        {
            using var response = await _http.PostAsJsonAsync(_connection.TrackUrl, batch).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
                _logger.LogDebug("telemetry: ingestion answered {Status}; dropped", (int)response.StatusCode);
        }
        catch (Exception e)
        {
            _logger.LogDebug("telemetry: send failed ({Type}); dropped", e.GetType().Name);
        }
    }
}
