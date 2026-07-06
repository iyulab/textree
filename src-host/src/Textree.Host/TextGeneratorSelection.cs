namespace Textree.Host;

/// <summary>
/// Pure decision of which generator backend to register, extracted so the branch condition is
/// unit-testable without booting the host. `null` means "no BYO configured" -> LocalTextGenerator.
/// </summary>
public static class TextGeneratorSelection
{
    public static string? SelectedPreset(IDictionary<string, string?> env)
    {
        if (!env.TryGetValue("TEXTREE_BYO_PRESET", out var preset)) return null;
        return string.IsNullOrWhiteSpace(preset) ? null : preset;
    }
}
