using IronProw.Core;
using IronProw.IronHive;

namespace Textree.Host.Rag;

/// <summary>
/// Maps a BYO preset to the matching IronProw.IronHive bridge registration. One provider only;
/// the gateway's selection layer is inert for a single registration.
/// </summary>
public static class ByoProviderRegistration
{
    private const int Priority = 100;

    public static IronProwBuilder Register(
        IronProwBuilder builder, string preset, string baseUrl, string? apiKey, string model)
    {
        // The OpenAI SDK wraps ApiKey in an ApiKeyCredential that throws on empty; unauthenticated
        // local servers (Ollama) don't check it, so pass a placeholder when blank.
        string key = string.IsNullOrEmpty(apiKey) ? "not-required" : apiKey;

        return preset.ToLowerInvariant() switch
        {
            "openai" => builder.AddIronHiveOpenAI("byo", Priority, model, c => { c.ApiKey = key; if (!string.IsNullOrEmpty(baseUrl)) c.BaseUrl = baseUrl; }),
            "anthropic" => builder.AddIronHiveAnthropic("byo", Priority, model, c => { c.ApiKey = key; if (!string.IsNullOrEmpty(baseUrl)) c.BaseUrl = baseUrl; }),
            // GoogleAIConfig has no direct BaseUrl; the endpoint override lives on HttpOptions.
            "gemini" => builder.AddIronHiveGoogleAI("byo", Priority, model, c => { c.ApiKey = key; if (!string.IsNullOrEmpty(baseUrl)) c.HttpOptions = new Google.GenAI.Types.HttpOptions { BaseUrl = baseUrl }; }),
            "gpustack" => builder.AddIronHiveGpuStack("byo", Priority, model, c => { c.ApiKey = key; c.BaseUrl = baseUrl; }),
            // grok, custom, ollama, and any unknown preset → generic OpenAI-compatible.
            _ => builder.AddIronHiveOpenAICompatible("byo", Priority, model, c => { c.ApiKey = key; c.BaseUrl = baseUrl; }),
        };
    }
}
