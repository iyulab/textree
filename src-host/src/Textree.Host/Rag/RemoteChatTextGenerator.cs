using System.Runtime.CompilerServices;
using System.Text;
using IronHive.Abstractions.Messages;
using IronHive.Core.Microsoft;
using IronHive.Providers.OpenAI.Compatible;
using IronHive.Providers.OpenAI.Compatible.GpuStack;
using IronProw.LMSupply;
using Microsoft.Extensions.AI;
using MeaiChatMessage = Microsoft.Extensions.AI.ChatMessage;

namespace Textree.Host.Rag;

// ── BYO generator backend (Phase D, 2026-07-06) ────────────────────────────────
// Borrows the ironhive family (IronHive.Core + IronHive.Providers.OpenAI.Compatible) the same
// way LocalTextGenerator borrows IronProw.LMSupply — a lightweight 2-decorator wiring, not the
// full IronProw.Core selection/gateway builder (AddIronProw()), which is inert for a single
// always-on provider (same reasoning D12 used to reject AddLMSupplyLocal for the local path).
// GpuStackMessageGenerator/OpenAICompatibleMessageGenerator both implement IMessageGenerator;
// ChatClientAdapter bridges that to the standard Microsoft.Extensions.AI.IChatClient, which then
// slots into the exact same LocalSafetyChatClient wrapping LocalTextGenerator already uses.

/// <summary>
/// Text generator backed by a user-configured OpenAI-compatible server (Ollama, GPUStack, or a
/// custom endpoint). Unlike <see cref="LocalTextGenerator"/> there is no model-download step:
/// the server is assumed already running, so <see cref="Ready"/> reflects "configured", not
/// "reachability proven" — a bad endpoint surfaces its failure on the first real
/// <see cref="GenerateAsync"/> call, recorded into <see cref="LastError"/>.
/// </summary>
public sealed class RemoteChatTextGenerator : ITextGenerator, IAsyncDisposable
{
    private const int RepetitionTail = 256;

    private readonly string _preset;
    private readonly string _baseUrl;
    private readonly string? _apiKey;
    private readonly string _model;
    private string? _lastError;

    public RemoteChatTextGenerator(string preset, string baseUrl, string? apiKey, string model)
    {
        _preset = preset;
        _baseUrl = baseUrl;
        _apiKey = apiKey;
        _model = model;
    }

    // No load step for a remote provider — the endpoint is either reachable or not, discovered
    // lazily on first request (mirrors ironhive's own Config classes, which resolve BaseUrl/ApiKey
    // per-call rather than up front). "Ready" here means "configured", not "verified".
    public bool Ready => true;
    public string? LastError => Volatile.Read(ref _lastError);

    public Task PrepareAsync(CancellationToken ct) => Task.CompletedTask;

    private IMessageGenerator BuildGenerator() => _preset.ToLowerInvariant() switch
    {
        "gpustack" => new GpuStackMessageGenerator(new GpuStackConfig { BaseUrl = _baseUrl, ApiKey = EffectiveApiKey }),
        _ => new OpenAICompatibleMessageGenerator(new OpenAICompatibleConfig { BaseUrl = _baseUrl, ApiKey = EffectiveApiKey }),
    };

    // The underlying OpenAI SDK client (constructed inside ironhive's OpenAIClientFactory) wraps
    // whatever ApiKey string it gets in an ApiKeyCredential, which throws ArgumentException on
    // null/empty — even though most self-hosted OpenAI-compatible servers (Ollama in particular)
    // do not check the Authorization header at all. Substitute a non-empty placeholder so an
    // unauthenticated local server is reachable without forcing the caller to invent a fake key.
    private string EffectiveApiKey => string.IsNullOrEmpty(_apiKey) ? "not-required" : _apiKey;

    public async IAsyncEnumerable<string> GenerateAsync(
        IReadOnlyList<ChatMessage> messages,
        GenerationOptions opts,
        [EnumeratorCancellation] CancellationToken ct)
    {
        using var generator = BuildGenerator();
        using var chat = new LocalSafetyChatClient(
            new ChatClientAdapter(generator, _model, _preset),
            new LocalSafetyOptions { DefaultMaxOutputTokens = 512 },
            new LazyReadinessProbe(() => true, new[] { _model }));

        var chatMessages = messages.Select(ToChatMessage).ToList();
        var chatOptions = new ChatOptions
        {
            MaxOutputTokens = opts.MaxTokens,
            Temperature = opts.Temperature,
        };

        var recent = new StringBuilder(RepetitionTail);
        IAsyncEnumerator<ChatResponseUpdate>? enumerator = null;
        try
        {
            enumerator = chat.GetStreamingResponseAsync(chatMessages, chatOptions, ct).GetAsyncEnumerator(ct);
            while (true)
            {
                bool moved;
                try
                {
                    moved = await enumerator.MoveNextAsync();
                }
                catch (Exception ex) when (ex is not OperationCanceledException)
                {
                    // Connection refused / timeout / server error — record for the next /health
                    // poll so the frontend's existing host-error UI (chatStore.errorMessage) can
                    // show it instead of the stream just going silently dark.
                    Volatile.Write(ref _lastError, ex.Message);
                    throw;
                }
                if (!moved) yield break;

                var update = enumerator.Current;
                ct.ThrowIfCancellationRequested();
                if (string.IsNullOrEmpty(update.Text)) continue;
                yield return update.Text;

                recent.Append(update.Text);
                if (recent.Length > RepetitionTail) recent.Remove(0, recent.Length - RepetitionTail);
                if (RepetitionGuard.IsDegenerate(recent.ToString())) yield break;
            }
        }
        finally
        {
            if (enumerator is not null) await enumerator.DisposeAsync();
        }
    }

    private static MeaiChatMessage ToChatMessage(Rag.ChatMessage m) =>
        new(ParseRole(m.Role), m.Content);

    private static ChatRole ParseRole(string? role)
    {
        if (string.IsNullOrWhiteSpace(role))
            throw new ArgumentException("Message role must not be null or empty.", nameof(role));

        return role.ToLowerInvariant() switch
        {
            "system" => ChatRole.System,
            "assistant" => ChatRole.Assistant,
            "tool" => ChatRole.Tool,
            _ => ChatRole.User,
        };
    }

    public ValueTask DisposeAsync() => ValueTask.CompletedTask;
}
