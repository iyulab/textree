using IronProw.Core;
using IronProw.IronHive;

namespace Textree.Host.Rag;

/// <summary>
/// Turns the BYO settings the desktop app hands over into an IronProw.IronHive endpoint and registers
/// it. The preset catalogue — default addresses, what each preset requires, which provider serves it
/// — belongs to <see cref="ByoPresets"/>; this class only adapts textree's settings onto it. One
/// provider only; the gateway's selection layer is inert for a single registration.
/// </summary>
public static class ByoProviderRegistration
{
    private const int Priority = 100;

    /// <summary>
    /// The endpoint for what the settings hold. A preset id the catalogue does not know is read as
    /// <c>custom</c> (any OpenAI-compatible server), which is how such ids were always served; the
    /// catalogue then asks for the base URL that preset needs instead of guessing a wire.
    /// </summary>
    public static ByoEndpoint EndpointFor(string preset, string? baseUrl, string? apiKey) =>
        new(ByoPresets.Find(preset)?.Id ?? ByoPresets.Custom.Id, baseUrl ?? "", apiKey ?? "");

    /// <summary>Registers <paramref name="endpoint"/>, which must have passed <see cref="ByoPresets.Validate"/>.</summary>
    public static IronProwBuilder Register(IronProwBuilder builder, ByoEndpoint endpoint, string model) =>
        builder.AddIronHiveByo("byo", Priority, model, endpoint);
}
