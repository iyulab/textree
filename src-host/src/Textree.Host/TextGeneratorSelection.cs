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

    /// <summary>
    /// Default /chat token cap when the request doesn't specify one. 512 exists to protect the
    /// bundled CPU model from unbounded generation; that rationale doesn't apply to a BYO backend
    /// (user's own server or a paid frontier API), where a low cap actively hurts — thinking
    /// models (e.g. qwen3) spend reasoning tokens against it and can return an empty answer.
    /// </summary>
    public static int DefaultMaxTokens(string? preset) => preset is null ? 512 : 4096;
}
