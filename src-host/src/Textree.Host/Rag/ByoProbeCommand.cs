using System.Text.Json;
using IronProw.IronHive;

namespace Textree.Host.Rag;

/// <summary>
/// <c>textree-host probe-byo</c>: checks the BYO settings in the environment for real and prints the
/// outcome as one JSON line, then exits — without starting the server. The desktop app runs it for
/// Settings ▸ Advanced ▸ Test connection, so the check never touches a running host and uses the same
/// catalogue (<see cref="ByoPresets.ProbeAsync"/>) the host registers providers from.
/// </summary>
/// <remarks>
/// The settings arrive the way the host's own do (<c>TEXTREE_BYO_PRESET</c>, <c>TEXTREE_BYO_BASE_URL</c>,
/// <c>TEXTREE_BYO_API_KEY</c>) — the key never appears on a command line. The exit code is 0 whenever a
/// result was printed, including a failed connection; only a usage error (no preset) is non-zero.
/// </remarks>
public static class ByoProbeCommand
{
    /// <summary>The first command-line argument that selects this command.</summary>
    public const string Name = "probe-byo";

    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);

    /// <summary>The line printed for each check.</summary>
    public sealed record Outcome(bool Ok, int ModelCount, int? Status, string? Error);

    /// <summary>Runs the check on <paramref name="env"/> and writes the outcome to <paramref name="output"/>.</summary>
    public static async Task<int> RunAsync(
        IDictionary<string, string?> env,
        TextWriter output,
        Func<ByoEndpoint, CancellationToken, Task<ByoProbeResult>>? probe = null,
        CancellationToken cancellationToken = default)
    {
        var preset = TextGeneratorSelection.SelectedPreset(env);
        if (preset is null)
        {
            await output.WriteLineAsync(Serialize(new Outcome(false, 0, null, "No AI server is selected.")));
            return 2;
        }

        env.TryGetValue("TEXTREE_BYO_BASE_URL", out var baseUrl);
        env.TryGetValue("TEXTREE_BYO_API_KEY", out var apiKey);
        var endpoint = ByoProviderRegistration.EndpointFor(preset, baseUrl, apiKey);
        probe ??= (e, ct) => ByoPresets.ProbeAsync(e, null, ct);
        var result = await probe(endpoint, cancellationToken);
        await output.WriteLineAsync(Serialize(new Outcome(result.Ok, result.ModelCount, result.StatusCode, result.Error)));
        return 0;
    }

    private static string Serialize(Outcome outcome) => JsonSerializer.Serialize(outcome, Json);
}
