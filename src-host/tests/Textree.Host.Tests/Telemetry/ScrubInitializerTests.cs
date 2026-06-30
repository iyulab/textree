using Microsoft.ApplicationInsights.DataContracts;
using Textree.Host.Telemetry;
using Xunit;

public class ScrubInitializerTests
{
    [Fact]
    public void Initialize_clears_identifying_context()
    {
        var t = new EventTelemetry("model.download.failed");
        t.Context.Cloud.RoleInstance = "DESKTOP-ABC\\alice"; // machine + username — must not survive
        t.Context.Device.Id = "device-guid";
        t.Context.User.Id = "user-123";
        t.Context.User.AuthenticatedUserId = "alice@example.com";
        t.Context.Location.Ip = "203.0.113.4";

        new ScrubInitializer().Initialize(t);

        Assert.True(string.IsNullOrEmpty(t.Context.Cloud.RoleInstance));
        Assert.True(string.IsNullOrEmpty(t.Context.Device.Id));
        Assert.True(string.IsNullOrEmpty(t.Context.User.Id));
        Assert.True(string.IsNullOrEmpty(t.Context.User.AuthenticatedUserId));
        Assert.True(string.IsNullOrEmpty(t.Context.Location.Ip));
    }
}
