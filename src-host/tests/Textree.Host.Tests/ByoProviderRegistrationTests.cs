using FluentAssertions;
using Microsoft.Extensions.DependencyInjection;
using IronProw.Core;
using IronProw.IronHive;
using Textree.Host.Rag;
using Xunit;

namespace Textree.Host.Tests;

public class ByoProviderRegistrationTests
{
    private static IProviderRegistry RegistryFor(string preset, string baseUrl, string model)
    {
        var services = new ServiceCollection();
        var builder = services.AddIronProw();
        ByoProviderRegistration.Register(builder, ByoProviderRegistration.EndpointFor(preset, baseUrl, "k"), model);
        return services.BuildServiceProvider().GetRequiredService<IProviderRegistry>();
    }

    [Theory]
    [InlineData("openai", ProviderKind.Frontier)]
    [InlineData("anthropic", ProviderKind.Frontier)]
    [InlineData("gemini", ProviderKind.Frontier)]
    [InlineData("grok", ProviderKind.Frontier)]
    [InlineData("gpustack", ProviderKind.Lan)]
    [InlineData("ollama", ProviderKind.Lan)]
    [InlineData("custom", ProviderKind.Lan)]
    public void Register_wires_one_candidate_of_expected_kind(string preset, ProviderKind kind)
    {
        RegistryFor(preset, "https://example.test/v1", "m")
            .GetOrdered().Should().ContainSingle(r => r.Kind == kind);
    }

    [Theory]
    [InlineData("anthropic")]
    [InlineData("openai")]
    [InlineData("ollama")]
    public void A_blank_base_url_uses_the_preset_default(string preset)
    {
        var endpoint = ByoProviderRegistration.EndpointFor(preset, "", "k");
        ByoPresets.Validate(endpoint).Should().BeNull();
        var act = () => RegistryFor(preset, "", "m");
        act.Should().NotThrow();
    }

    [Fact]
    public void Preset_ids_are_matched_without_regard_to_case()
    {
        ByoProviderRegistration.EndpointFor("GPUStack", "", null).Preset.Should().Be("gpustack");
    }

    [Fact]
    public void An_unknown_preset_is_served_as_a_custom_openai_compatible_server()
    {
        ByoProviderRegistration.EndpointFor("lmstudio", "http://localhost:1234/v1", null)
            .Preset.Should().Be("custom");
        RegistryFor("lmstudio", "http://localhost:1234/v1", "m")
            .GetOrdered().Should().ContainSingle(r => r.Kind == ProviderKind.Lan);
    }

    [Fact]
    public void A_frontier_preset_without_a_key_is_refused_before_reaching_a_provider()
    {
        ByoPresets.Validate(ByoProviderRegistration.EndpointFor("openai", "", null)).Should().NotBeNull();
    }

    [Fact]
    public void A_custom_server_without_an_address_is_refused()
    {
        ByoPresets.Validate(ByoProviderRegistration.EndpointFor("custom", "", null)).Should().NotBeNull();
    }

    [Fact]
    public async Task An_unusable_endpoint_reports_why_and_never_generates()
    {
        var gen = new UnusableTextGenerator("needs a key");
        gen.Ready.Should().BeFalse();
        gen.LastError.Should().Be("needs a key");
        var act = async () =>
        {
            await foreach (var _ in gen.GenerateAsync([], new GenerationOptions(), CancellationToken.None)) { }
        };
        await act.Should().ThrowAsync<InvalidOperationException>().WithMessage("needs a key");
    }
}
