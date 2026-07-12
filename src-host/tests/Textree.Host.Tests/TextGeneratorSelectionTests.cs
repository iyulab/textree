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

    [Fact]
    public void Local_backend_keeps_the_512_token_default()
    {
        Assert.Equal(512, TextGeneratorSelection.DefaultMaxTokens(null));
    }

    [Theory]
    [InlineData("ollama")]
    [InlineData("gpustack")]
    [InlineData("openai")]
    [InlineData("anthropic")]
    [InlineData("gemini")]
    [InlineData("grok")]
    [InlineData("custom")]
    public void Byo_backend_raises_the_default_to_4096(string preset)
    {
        Assert.Equal(4096, TextGeneratorSelection.DefaultMaxTokens(preset));
    }
}
