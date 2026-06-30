using Textree.Host.Telemetry;
using Xunit;

public class TelemetryOptionsTests
{
    [Fact]
    public void Read_without_connection_string_is_disabled()
    {
        var opts = TelemetryOptions.Read(new Dictionary<string, string?>());
        Assert.False(opts.IsEnabled);
        Assert.Null(opts.ConnectionString);
    }

    [Fact]
    public void Read_with_blank_connection_string_is_disabled()
    {
        var env = new Dictionary<string, string?> { [TelemetryOptions.EnvVar] = "   " };
        Assert.False(TelemetryOptions.Read(env).IsEnabled);
    }

    [Fact]
    public void Read_with_connection_string_is_enabled()
    {
        var env = new Dictionary<string, string?>
        {
            [TelemetryOptions.EnvVar] = "InstrumentationKey=abc;IngestionEndpoint=https://x/",
        };
        var opts = TelemetryOptions.Read(env);
        Assert.True(opts.IsEnabled);
        Assert.Equal("InstrumentationKey=abc;IngestionEndpoint=https://x/", opts.ConnectionString);
    }
}
