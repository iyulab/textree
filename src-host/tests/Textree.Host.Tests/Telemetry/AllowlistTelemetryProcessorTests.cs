// src-host/tests/Textree.Host.Tests/Telemetry/AllowlistTelemetryProcessorTests.cs
using Microsoft.ApplicationInsights.Channel;
using Microsoft.ApplicationInsights.DataContracts;
using Microsoft.ApplicationInsights.Extensibility;
using Textree.Host.Telemetry;
using Xunit;

public class AllowlistTelemetryProcessorTests
{
    private sealed class RecordingNext : ITelemetryProcessor
    {
        public List<ITelemetry> Passed { get; } = new();
        public void Process(ITelemetry item) => Passed.Add(item);
    }

    [Fact]
    public void Allowlisted_event_passes_through()
    {
        var next = new RecordingNext();
        var proc = new AllowlistTelemetryProcessor(next);
        proc.Process(new EventTelemetry(TelemetryEventName.ModelDownloadFailed));
        Assert.Single(next.Passed);
    }

    [Fact]
    public void Unknown_event_name_is_dropped()
    {
        var next = new RecordingNext();
        var proc = new AllowlistTelemetryProcessor(next);
        proc.Process(new EventTelemetry("something.we.never.defined"));
        Assert.Empty(next.Passed);
    }

    // The property that matters: an UNKNOWN telemetry TYPE (auto-collected request/exception/trace,
    // carrying paths, machine name, full stacks) must fail closed — proven via the default, not a list.
    [Fact]
    public void Auto_collected_telemetry_types_are_dropped()
    {
        var next = new RecordingNext();
        var proc = new AllowlistTelemetryProcessor(next);

        var req = new RequestTelemetry { Name = "POST /index", Url = new Uri("https://host/index?vault=C:/Users/alice/notes") };
        var ex = new ExceptionTelemetry(new IOException("No such file ... 일기/x.md")); // path-bearing
        var trace = new TraceTelemetry("loading C:/Users/alice/.cache/model.onnx");

        proc.Process(req);
        proc.Process(ex);
        proc.Process(trace);

        Assert.Empty(next.Passed); // none of these are allowlisted EventTelemetry → all dropped
    }
}
