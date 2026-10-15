using System.Text.Json;
using FluentAssertions;
using IronProw.IronHive;
using Textree.Host.Rag;
using Xunit;

namespace Textree.Host.Tests;

public class ByoProbeCommandTests
{
    private static async Task<(int Code, JsonElement Line, ByoEndpoint? Seen)> Run(
        Dictionary<string, string?> env, ByoProbeResult answer)
    {
        ByoEndpoint? seen = null;
        var output = new StringWriter();
        var code = await ByoProbeCommand.RunAsync(env, output, (e, _) =>
        {
            seen = e;
            return Task.FromResult(answer);
        });
        var lines = output.ToString().Split('\n', StringSplitOptions.RemoveEmptyEntries);
        lines.Should().ContainSingle();
        return (code, JsonDocument.Parse(lines[0]).RootElement, seen);
    }

    [Fact]
    public async Task A_reachable_server_prints_ok_and_the_model_count()
    {
        var (code, line, seen) = await Run(
            new() { ["TEXTREE_BYO_PRESET"] = "openai", ["TEXTREE_BYO_API_KEY"] = "sk-x" },
            new ByoProbeResult(true, Enumerable.Range(0, 141).Select(i => $"model-{i}").ToArray(), null, null));

        code.Should().Be(0);
        line.GetProperty("ok").GetBoolean().Should().BeTrue();
        line.GetProperty("modelCount").GetInt32().Should().Be(141);
        seen!.ApiKey.Should().Be("sk-x");
    }

    [Fact]
    public async Task A_refused_key_prints_the_status_and_still_exits_zero()
    {
        var (code, line, _) = await Run(
            new() { ["TEXTREE_BYO_PRESET"] = "anthropic", ["TEXTREE_BYO_API_KEY"] = "bad" },
            new ByoProbeResult(false, [], 401, "Unauthorized"));

        code.Should().Be(0);
        line.GetProperty("ok").GetBoolean().Should().BeFalse();
        line.GetProperty("status").GetInt32().Should().Be(401);
        line.GetProperty("error").GetString().Should().Be("Unauthorized");
    }

    [Fact]
    public async Task An_unknown_preset_is_checked_as_custom_with_its_base_url()
    {
        var (_, _, seen) = await Run(
            new() { ["TEXTREE_BYO_PRESET"] = "my-server", ["TEXTREE_BYO_BASE_URL"] = "http://lan:8080/v1" },
            new ByoProbeResult(true, ["model-0"], null, null));

        seen!.Preset.Should().Be(ByoPresets.Custom.Id);
        seen.BaseUrl.Should().Be("http://lan:8080/v1");
    }

    [Fact]
    public async Task No_preset_is_a_usage_error_and_contacts_nothing()
    {
        var called = false;
        var output = new StringWriter();
        var code = await ByoProbeCommand.RunAsync(new Dictionary<string, string?>(), output, (_, _) =>
        {
            called = true;
            return Task.FromResult(new ByoProbeResult(true, [], null, null));
        });

        code.Should().Be(2);
        called.Should().BeFalse();
        JsonDocument.Parse(output.ToString()).RootElement.GetProperty("ok").GetBoolean().Should().BeFalse();
    }
}
