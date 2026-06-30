using Microsoft.ApplicationInsights;
using Microsoft.ApplicationInsights.Channel;
using Microsoft.ApplicationInsights.Extensibility;
using Microsoft.Extensions.Logging;
using Textree.Host.Rag;

namespace Textree.Host.Telemetry;

/// <summary>
/// Builds the outbound pipe and emits allowlist error events. Only the exception TYPE NAME crosses
/// into the payload — never the message or stack. Every send is mirrored to the local logger so the
/// (mandatory, no-opt-out) reporting is visible, not silent.
/// </summary>
public sealed class TelemetryEmitter : ITelemetryEmitter
{
    private readonly TelemetryClient _client;
    private readonly ILogger _logger;
    private readonly EnvFacts _env;

    public TelemetryEmitter(TelemetryClient client, ILogger logger, EnvFacts env)
    {
        _client = client;
        _logger = logger;
        _env = env;
    }

    public static ITelemetryEmitter Create(
        TelemetryOptions options, ILogger logger, EnvFacts env, ITelemetryChannel? channel = null)
    {
        // Fail-closed: disabled, or enabled-but-no-endpoint, both yield the no-op emitter.
        if (!options.IsEnabled || string.IsNullOrWhiteSpace(options.ConnectionString))
            return new NullTelemetryEmitter();

        // Hand-built config: no default initializers, InMemoryChannel (in-memory only, drops on
        // prolonged failure → offline = skip, no disk buffering). The optional channel lets tests
        // capture from the REAL production chain. Scrubbing lives in ContextScrubProcessor — NOT an
        // ITelemetryInitializer — because TelemetryClient.Initialize() re-stamps Cloud.RoleInstance
        // with the machine hostname AFTER initializers run; a processor runs later and overwrites it.
        // Hand-built config on purpose: do NOT switch to TelemetryConfiguration.CreateDefault() or the
        // Microsoft.ApplicationInsights.AspNetCore package — either reintroduces default initializers and
        // auto-collection (request/dependency/exception telemetry, machine-name context), defeating the
        // by-construction + allowlist + scrub design.
        var config = new TelemetryConfiguration
        {
            ConnectionString = options.ConnectionString,
            TelemetryChannel = channel ?? new InMemoryChannel(),
        };
        config.DefaultTelemetrySink.TelemetryProcessorChainBuilder
            .Use(next => new AllowlistTelemetryProcessor(next))
            .Use(next => new ContextScrubProcessor(next))
            .Build();

        return new TelemetryEmitter(new TelemetryClient(config), logger, env);
    }

    public void ReportError(string eventName, string? modelSlot, ModelPhase phase, Exception ex)
    {
        var props = TelemetryPayload.BuildErrorProperties(modelSlot, phase, ex.GetType().Name, _env);
        _client.TrackEvent(eventName, new Dictionary<string, string>(props));
        _logger.LogInformation("telemetry: {Event} sent (slot={Slot}, phase={Phase}, type={Type})",
            eventName, modelSlot ?? "-", phase, ex.GetType().Name);
    }
}
