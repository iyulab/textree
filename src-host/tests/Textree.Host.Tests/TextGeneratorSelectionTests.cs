using Textree.Host;
using Xunit;

namespace Textree.Host.Tests;

public sealed class TextGeneratorSelectionTests
{
    [Fact]
    public void No_preset_env_selects_local()
    {
        var env = new Dictionary<string, string?>();
        Assert.Null(TextGeneratorSelection.SelectedPreset(env));
    }

    [Fact]
    public void Empty_preset_env_selects_local()
    {
        var env = new Dictionary<string, string?> { ["TEXTREE_BYO_PRESET"] = "" };
        Assert.Null(TextGeneratorSelection.SelectedPreset(env));
    }

    [Fact]
    public void Whitespace_preset_env_selects_local()
    {
        var env = new Dictionary<string, string?> { ["TEXTREE_BYO_PRESET"] = "   " };
        Assert.Null(TextGeneratorSelection.SelectedPreset(env));
    }

    [Theory]
    [InlineData("ollama")]
    [InlineData("gpustack")]
    [InlineData("custom")]
    [InlineData("GPUStack")] // case-insensitive — RemoteChatTextGenerator itself lowercases too
    public void Present_preset_env_selects_remote(string preset)
    {
        var env = new Dictionary<string, string?> { ["TEXTREE_BYO_PRESET"] = preset };
        Assert.Equal(preset, TextGeneratorSelection.SelectedPreset(env));
    }
}
