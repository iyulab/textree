// src-host/tests/Textree.Host.Tests/Telemetry/TelemetryEmitterTests.cs
using System.Net;
using System.Text.Json.Nodes;
using Microsoft.Extensions.Logging.Abstractions;
using Textree.Host.Rag;
using Textree.Host.Telemetry;
using Xunit;

public class TelemetryEmitterTests
{
    private const string Connection =
        "InstrumentationKey=00000000-0000-0000-0000-000000000000;IngestionEndpoint=https://ingest.example/";

    /// <summary>Captures the request the emitter really makes — URL, content type and body.</summary>
    private sealed class CapturingHandler(HttpStatusCode status = HttpStatusCode.OK, Exception? fail = null)
        : HttpMessageHandler
    {
        private readonly TaskCompletionSource<(Uri Url, string ContentType, string Body)> _sent =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        public int Calls;

        public Task<(Uri Url, string ContentType, string Body)> Sent => _sent.Task;

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
        {
            Interlocked.Increment(ref Calls);
            var body = await request.Content!.ReadAsStringAsync(ct);
            _sent.TrySetResult((request.RequestUri!, request.Content.Headers.ContentType!.MediaType!, body));
            if (fail is not null) throw fail;
            return new HttpResponseMessage(status)
            {
                Content = new StringContent("{\"itemsReceived\":1,\"itemsAccepted\":1,\"errors\":[]}"),
            };
        }
    }

    private static readonly EnvFacts Env = new("1.2.3", "Windows", "X64", "cpu8_ram16");

    private static ITelemetryEmitter Enabled(
        CapturingHandler handler, Microsoft.Extensions.Logging.ILogger? logger = null) =>
        TelemetryEmitter.Create(new TelemetryOptions(true, Connection), logger ?? NullLogger.Instance, Env, handler);

    private static async Task<JsonObject> SentEnvelope(CapturingHandler handler)
    {
        var (url, contentType, body) = await handler.Sent.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal("https://ingest.example/v2/track", url.ToString());
        Assert.Equal("application/json", contentType);
        return Assert.IsType<JsonObject>(Assert.Single(Assert.IsType<JsonArray>(JsonNode.Parse(body))));
    }

    [Fact]
    public void Disabled_emitter_is_noop()
    {
        var emitter = TelemetryEmitter.Create(new TelemetryOptions(false, null), NullLogger.Instance, Env);
        emitter.ReportError(TelemetryEventName.HostStartupFailed, null, ModelPhase.Error, new Exception("boom"));
        Assert.IsType<NullTelemetryEmitter>(emitter);
    }

    [Fact]
    public void A_connection_string_without_an_endpoint_sends_nothing()
    {
        var emitter = TelemetryEmitter.Create(
            new TelemetryOptions(true, "InstrumentationKey=00000000-0000-0000-0000-000000000000"),
            NullLogger.Instance, Env, new CapturingHandler());
        Assert.IsType<NullTelemetryEmitter>(emitter);
    }

    [Fact]
    public async Task The_envelope_holds_exactly_the_written_fields_and_no_message_or_identity()
    {
        var handler = new CapturingHandler();
        Enabled(handler).ReportError(
            TelemetryEventName.ModelDownloadFailed, "embedder", ModelPhase.Downloading,
            new IOException($"No such file or directory: C:/Users/{Environment.UserName}/.cache/café.onnx"));

        var envelope = await SentEnvelope(handler);
        var raw = envelope.ToJsonString();

        Assert.Equal(new[] { "name", "time", "iKey", "tags", "data" }, envelope.Select(p => p.Key));
        Assert.Equal("Microsoft.ApplicationInsights.Event", (string?)envelope["name"]);
        var tags = envelope["tags"]!.AsObject();
        Assert.Equal(new[] { "ai.cloud.role", "ai.application.ver" }, tags.Select(p => p.Key));
        Assert.Equal("textree-host", (string?)tags["ai.cloud.role"]);
        Assert.Equal("1.2.3", (string?)tags["ai.application.ver"]);

        Assert.Equal("EventData", (string?)envelope["data"]!["baseType"]);
        var baseData = envelope["data"]!["baseData"]!.AsObject();
        Assert.Equal(TelemetryEventName.ModelDownloadFailed, (string?)baseData["name"]);
        var props = baseData["properties"]!.AsObject();
        Assert.Equal(
            new[] { "app_version", "arch", "exception_type", "hardware_class", "model_slot", "os", "phase" },
            props.Select(p => p.Key).Order(StringComparer.Ordinal));
        Assert.Equal("IOException", (string?)props["exception_type"]);

        // Nothing identifying and nothing from the exception message, anywhere in what was sent.
        Assert.DoesNotContain(Environment.MachineName, raw, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain(Environment.UserName, raw, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain(".cache", raw);
        Assert.DoesNotContain("café", raw);
    }

    [Fact]
    public async Task An_event_not_on_the_allowlist_is_not_sent()
    {
        var handler = new CapturingHandler();
        Enabled(handler).ReportError("note.opened", null, ModelPhase.Error, new Exception("boom"));
        await Task.Delay(200);
        Assert.Equal(0, handler.Calls);
    }

    [Fact]
    public async Task A_failed_or_rejected_send_is_dropped_without_throwing()
    {
        var offline = new CapturingHandler(fail: new HttpRequestException("offline"));
        Enabled(offline).ReportError(TelemetryEventName.HostStartupFailed, null, ModelPhase.Error, new Exception("boom"));
        await offline.Sent.WaitAsync(TimeSpan.FromSeconds(5));

        var rejected = new CapturingHandler(HttpStatusCode.BadRequest);
        Enabled(rejected).ReportError(TelemetryEventName.HostStartupFailed, null, ModelPhase.Error, new Exception("boom"));
        await rejected.Sent.WaitAsync(TimeSpan.FromSeconds(5));
    }

    private sealed class ListLogger : Microsoft.Extensions.Logging.ILogger
    {
        public List<string> Messages { get; } = new();
        public IDisposable? BeginScope<TState>(TState state) where TState : notnull => null;
        public bool IsEnabled(Microsoft.Extensions.Logging.LogLevel logLevel) =>
            logLevel >= Microsoft.Extensions.Logging.LogLevel.Information;
        public void Log<TState>(Microsoft.Extensions.Logging.LogLevel logLevel, Microsoft.Extensions.Logging.EventId eventId,
            TState state, Exception? exception, Func<TState, Exception?, string> formatter)
        {
            if (IsEnabled(logLevel)) lock (Messages) Messages.Add(formatter(state, exception));
        }
    }

    [Fact]
    public async Task Each_send_is_mirrored_to_the_local_log_without_the_message()
    {
        var logger = new ListLogger();
        var handler = new CapturingHandler();
        Enabled(handler, logger).ReportError(
            TelemetryEventName.ModelDownloadFailed, "embedder", ModelPhase.Downloading,
            new IOException("No such file: C:/Users/alice/café.onnx"));
        await handler.Sent.WaitAsync(TimeSpan.FromSeconds(5));

        var line = Assert.Single(logger.Messages);
        Assert.Contains(TelemetryEventName.ModelDownloadFailed, line);
        Assert.DoesNotContain("alice", line);
        Assert.DoesNotContain("café", line);
    }
}
