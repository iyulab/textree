using FluentAssertions;
using Microsoft.Extensions.DependencyInjection;
using IronProw.Core;
using Textree.Host.Rag;
using Xunit;

namespace Textree.Host.Tests;

public class ByoProviderRegistrationTests
{
    private static IProviderRegistry RegistryFor(string preset, string baseUrl, string model)
    {
        var services = new ServiceCollection();
        var builder = services.AddIronProw();
        ByoProviderRegistration.Register(builder, preset, baseUrl, "k", model);
        return services.BuildServiceProvider().GetRequiredService<IProviderRegistry>();
    }

    [Theory]
    [InlineData("openai", ProviderKind.Frontier)]
    [InlineData("anthropic", ProviderKind.Frontier)]
    [InlineData("gemini", ProviderKind.Frontier)]
    [InlineData("grok", ProviderKind.Lan)]        // grok → OpenAI-compatible (Lan)
    [InlineData("gpustack", ProviderKind.Lan)]
    [InlineData("ollama", ProviderKind.Lan)]
    [InlineData("custom", ProviderKind.Lan)]
    public void Register_wires_one_candidate_of_expected_kind(string preset, ProviderKind kind)
    {
        RegistryFor(preset, "https://example.test/v1", "m")
            .GetOrdered().Should().ContainSingle(r => r.Kind == kind);
    }

    [Fact]
    public void Register_anthropic_with_blank_base_url_does_not_throw()
    {
        var act = () => RegistryFor("anthropic", "", "claude-opus-4-5");
        act.Should().NotThrow();
    }
}
