using Textree.Host.Rag;

namespace Textree.Host.Telemetry;

/// <summary>
/// Assembles error-event properties FROM KNOWN-GOOD STRUCTURED FIELDS ONLY. The signature accepts
/// no free text (an exception *type name*, not the exception), so content/PII has no entry path.
/// This is the fail-closed inverse of the frontend friendlyError (which deliberately passes raw text through).
/// </summary>
public static class TelemetryPayload
{
    public static IReadOnlyDictionary<string, string> BuildErrorProperties(
        string? modelSlot, ModelPhase phase, string exceptionType, EnvFacts env)
    {
        var props = new Dictionary<string, string>
        {
            ["phase"] = phase.ToString(),
            ["exception_type"] = SanitizeTypeName(exceptionType),
            ["app_version"] = env.AppVersion,
            ["os"] = env.Os,
            ["arch"] = env.Arch,
            ["hardware_class"] = env.HardwareClass,
        };
        if (modelSlot is not null) props["model_slot"] = modelSlot;
        return props;
    }

    // Defense in depth: exception_type must be a type name (callers pass ex.GetType().Name).
    // Keep only the leading non-whitespace token so a message/path can never ride through.
    // Never throws — telemetry must not break the app.
    private static string SanitizeTypeName(string value)
    {
        var trimmed = value.AsSpan().TrimStart();
        var space = trimmed.IndexOfAny(' ', '\t');
        return space >= 0 ? trimmed[..space].ToString() : trimmed.ToString();
    }

    public static string EventForModelFailure(ModelPhase phaseAtFailure) => phaseAtFailure switch
    {
        ModelPhase.Downloading => TelemetryEventName.ModelDownloadFailed,
        ModelPhase.Loading => TelemetryEventName.ModelLoadFailed,
        _ => TelemetryEventName.EmbedderInitFailed,
    };
}
