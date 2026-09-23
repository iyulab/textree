using Textree.Host.Telemetry;
using Xunit;

public class TelemetryConnectionTests
{
    [Fact]
    public void Key_and_endpoint_give_the_track_url()
    {
        var c = TelemetryConnection.Parse(
            "InstrumentationKey=abc;IngestionEndpoint=https://region.in.example.com/;LiveEndpoint=https://live/")!;
        Assert.Equal("abc", c.InstrumentationKey);
        Assert.Equal("https://region.in.example.com/v2/track", c.TrackUrl.ToString());
    }

    [Fact]
    public void An_endpoint_without_a_trailing_slash_gives_the_same_url()
    {
        Assert.Equal(
            "https://x.example.com/v2/track",
            TelemetryConnection.Parse("InstrumentationKey=abc;IngestionEndpoint=https://x.example.com")!.TrackUrl.ToString());
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("garbage")]
    [InlineData("IngestionEndpoint=https://x/")]
    [InlineData("InstrumentationKey=abc")]
    [InlineData("InstrumentationKey=;IngestionEndpoint=https://x/")]
    [InlineData("InstrumentationKey=abc;IngestionEndpoint=not a url")]
    [InlineData("InstrumentationKey=abc;IngestionEndpoint=file:///etc")]
    public void Anything_less_than_both_disables_telemetry(string? raw)
    {
        Assert.Null(TelemetryConnection.Parse(raw));
    }
}
