using TauriKit.Sidecar.Loopback;

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
    public const int MinimumLength = LoopbackCredentials.MinimumLength;

    /// <summary>The caller kind the app's token is registered as — the only caller there is.</summary>
    public const string Caller = "app";

    /// <summary>
    /// What the host prints on standard output, followed by its port, once it listens. The host
    /// takes a free port on 127.0.0.1 itself, so nothing can take it between choosing and binding.
    /// </summary>
    public const string ReadyPrefix = "textree-host ready port=";
}
