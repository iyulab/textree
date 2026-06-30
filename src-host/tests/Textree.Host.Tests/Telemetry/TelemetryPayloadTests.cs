using Textree.Host.Rag;
using Textree.Host.Telemetry;
using Xunit;

public class TelemetryPayloadTests
{
    private static readonly EnvFacts Env = new("1.2.3", "Windows", "X64", "cpu8_ram16");

    [Fact]
    public void BuildErrorProperties_contains_only_allowlist_keys()
    {
        var props = TelemetryPayload.BuildErrorProperties("embedder", ModelPhase.Downloading, "HttpRequestException", Env);
        var allowed = new HashSet<string>
        {
            "model_slot", "phase", "exception_type", "app_version", "os", "arch", "hardware_class",
        };
        Assert.All(props.Keys, k => Assert.Contains(k, allowed));
    }

    [Fact]
    public void BuildErrorProperties_carries_structured_values_verbatim()
    {
        var props = TelemetryPayload.BuildErrorProperties("generator", ModelPhase.Loading, "IOException", Env);
        Assert.Equal("generator", props["model_slot"]);
        Assert.Equal("Loading", props["phase"]);
        Assert.Equal("IOException", props["exception_type"]);
        Assert.Equal("1.2.3", props["app_version"]);
        Assert.Equal("Windows", props["os"]);
        Assert.Equal("X64", props["arch"]);
        Assert.Equal("cpu8_ram16", props["hardware_class"]);
    }

    [Fact]
    public void BuildErrorProperties_omits_model_slot_when_null()
    {
        var props = TelemetryPayload.BuildErrorProperties(null, ModelPhase.Error, "Exception", Env);
        Assert.False(props.ContainsKey("model_slot"));
    }

    [Theory]
    [InlineData(ModelPhase.Downloading, TelemetryEventName.ModelDownloadFailed)]
    [InlineData(ModelPhase.Loading, TelemetryEventName.ModelLoadFailed)]
    [InlineData(ModelPhase.Idle, TelemetryEventName.EmbedderInitFailed)]
    public void EventForModelFailure_maps_phase_to_event(ModelPhase phase, string expected)
    {
        Assert.Equal(expected, TelemetryPayload.EventForModelFailure(phase));
    }

    [Fact]
    public void EnvFacts_Current_produces_nonempty_safe_facts()
    {
        var env = EnvFacts.Current();
        Assert.False(string.IsNullOrWhiteSpace(env.AppVersion));
        Assert.False(string.IsNullOrWhiteSpace(env.Os));
        Assert.False(string.IsNullOrWhiteSpace(env.Arch));
        Assert.StartsWith("cpu", env.HardwareClass);
    }

    [Fact]
    public void BuildErrorProperties_sanitizes_freetext_exception_type()
    {
        var props = TelemetryPayload.BuildErrorProperties(
            "embedder", ModelPhase.Error, "No such file: /Users/alice/일기.md", Env);
        Assert.DoesNotContain(' ', props["exception_type"]);
        Assert.DoesNotContain("alice", props["exception_type"]);
        Assert.DoesNotContain("일기", props["exception_type"]);
    }
}
