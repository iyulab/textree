using Microsoft.ApplicationInsights.Channel;
using Microsoft.ApplicationInsights.DataContracts;
using Microsoft.ApplicationInsights.Extensibility;
using Textree.Host.Telemetry;
using Xunit;

public class ContextScrubProcessorTests
{
    private sealed class RecordingNext : ITelemetryProcessor
    {
        public ITelemetry? Last { get; private set; }
        public void Process(ITelemetry item) => Last = item;
    }

    [Fact]
    public void Process_clears_identifying_context_then_forwards()
    {
        var next = new RecordingNext();
        var t = new EventTelemetry("model.download.failed");
        t.Context.Cloud.RoleInstance = "DESKTOP-ABC\\alice";
        t.Context.Cloud.RoleName = "textree-host";
        t.Context.Device.Id = "device-guid";
        t.Context.User.Id = "user-123";
        t.Context.User.AuthenticatedUserId = "alice@example.com";
        t.Context.Location.Ip = "203.0.113.4";

        new ContextScrubProcessor(next).Process(t);

        Assert.Same(t, next.Last);
        Assert.True(string.IsNullOrEmpty(t.Context.Cloud.RoleInstance));
        Assert.True(string.IsNullOrEmpty(t.Context.Cloud.RoleName));
        Assert.True(string.IsNullOrEmpty(t.Context.Device.Id));
        Assert.True(string.IsNullOrEmpty(t.Context.User.Id));
        Assert.True(string.IsNullOrEmpty(t.Context.User.AuthenticatedUserId));
        Assert.True(string.IsNullOrEmpty(t.Context.Location.Ip));
    }
}
