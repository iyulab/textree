namespace Textree.Host;

/// <summary>
/// The per-run token the desktop app hands the host, which every request must carry as
/// <c>Authorization: Bearer &lt;token&gt;</c>.
/// </summary>
public static class HostToken
{
    /// <summary>The environment variable (and configuration key) the token arrives in.</summary>
    public const string Setting = "TEXTREE_HOST_TOKEN";

    /// <summary>A shorter token is a mistake, not a secret.</summary>
    public const int MinimumLength = 32;
}
