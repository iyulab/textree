// src-host/tests/Textree.Host.Tests/Telemetry/TelemetryEmitterTests.cs
using Microsoft.ApplicationInsights;
using Microsoft.ApplicationInsights.Channel;
using Microsoft.ApplicationInsights.DataContracts;
using Microsoft.ApplicationInsights.Extensibility;
using Microsoft.Extensions.Logging.Abstractions;
using Textree.Host.Rag;
using Textree.Host.Telemetry;
using Xunit;

public class TelemetryEmitterTests
{
    private sealed class CapturingChannel : ITelemetryChannel
    {
        public List<ITelemetry> Sent { get; } = new();
        public bool? DeveloperMode { get; set; }
        public string? EndpointAddress { get; set; }
        public void Send(ITelemetry item) => Sent.Add(item);
        public void Flush() { }
        public void Dispose() { }
    }

    private static readonly EnvFacts Env = new("1.2.3", "Windows", "X64", "cpu8_ram16");

    [Fact]
    public void Disabled_emitter_is_noop()
    {
        var emitter = TelemetryEmitter.Create(new TelemetryOptions(false, null), NullLogger.Instance, Env);
        // Must not throw and must send nothing — there is no channel to assert against, so the
        // contract is simply "no exception, returns a usable instance".
        emitter.ReportError(TelemetryEventName.HostStartupFailed, null, ModelPhase.Error, new Exception("boom"));
        Assert.IsType<NullTelemetryEmitter>(emitter);
    }

    [Fact]
    public void Enabled_emitter_sends_allowlist_event_without_message_or_pii()
    {
        var channel = new CapturingChannel();
        var emitter = TelemetryEmitter.Create(
            new TelemetryOptions(true, "InstrumentationKey=00000000-0000-0000-0000-000000000000"),
            NullLogger.Instance, Env, channel);

        emitter.ReportError(
            TelemetryEventName.ModelDownloadFailed, "embedder", ModelPhase.Downloading,
            new IOException("No such file or directory: C:/Users/alice/.cache/일기.onnx"));

        var evt = Assert.IsType<EventTelemetry>(Assert.Single(channel.Sent));
        Assert.Equal(TelemetryEventName.ModelDownloadFailed, evt.Name);
        Assert.Equal("IOException", evt.Properties["exception_type"]);
        Assert.Equal("embedder", evt.Properties["model_slot"]);
        // The exception MESSAGE (with the path) must appear in NO property value.
        Assert.DoesNotContain(evt.Properties.Values, v => v.Contains("alice") || v.Contains(".cache") || v.Contains("일기"));
        // Identifying context cleared by ContextScrubProcessor (after the SDK's post-init re-stamp).
        Assert.True(string.IsNullOrEmpty(evt.Context.Cloud.RoleInstance));
    }
}
