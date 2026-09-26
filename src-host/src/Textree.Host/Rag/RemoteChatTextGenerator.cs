using System.Runtime.CompilerServices;
using IronProw.Core;
using IronProw.LMSupply;
using Microsoft.Extensions.AI;
using MeaiChatMessage = Microsoft.Extensions.AI.ChatMessage;

namespace Textree.Host.Rag;

// ── BYO generator backend (Phase D, 2026-07-06; re-wired onto iron-prow's bridge) ──────────────
// Provider construction is no longer this class's job. Program.cs registers exactly one iron-prow
// provider via ByoProviderRegistration.Register (Task 5), which resolves to a DI-singleton
// IChatClient (iron-prow's SelectingChatClient — inert selection over a single candidate, same
// reasoning D12 used for the local path's lightweight wiring). This class only adapts that
// injected client into textree's ITextGenerator, keeping the streaming loop, the degeneration stop,
// and the LocalSafetyChatClient safety wrap that LocalTextGenerator already uses.

/// <summary>
/// Text generator backed by an injected iron-prow <see cref="IChatClient"/> (a single-provider,
/// inert <c>SelectingChatClient</c> built from the BYO preset). Unlike <see cref="LocalTextGenerator"/>
/// there is no model-download step: the endpoint is assumed already running, so <see cref="Ready"/>
/// reflects "configured", not "reachability proven" — a bad endpoint surfaces its failure on the
/// first real <see cref="GenerateAsync"/> call, recorded into <see cref="LastError"/>.
/// </summary>
public sealed class RemoteChatTextGenerator : ITextGenerator
{
    private readonly IChatClient _client;
    private readonly string _model;
    private string? _lastError;

    public RemoteChatTextGenerator(IChatClient client, string model)
    {
        _client = client;
        _model = model;
    }

    // No load step for a remote provider — the endpoint is either reachable or not, discovered
    // lazily on first request. "Ready" here means "configured", not "verified".
    public bool Ready => true;
    public string? LastError => Volatile.Read(ref _lastError);

    public Task PrepareAsync(CancellationToken ct) => Task.CompletedTask;

    public async IAsyncEnumerable<GenerationChunk> GenerateAsync(
        IReadOnlyList<ChatMessage> messages,
        GenerationOptions opts,
        [EnumeratorCancellation] CancellationToken ct)
    {
        // Keep textree's own safety wrap: iron-prow's default guard is a no-op NullGuard, so the
        // 512-token cap + readiness probe would otherwise be lost.
        //
        // Deliberately NOT wrapped in `using`/disposed: both wrappers (LocalSafetyChatClient and the
        // degeneration stop) extend M.E.AI's DelegatingChatClient, whose Dispose(bool) forwards to
        // InnerClient.Dispose(). _client here is a DI singleton (iron-prow's gateway, shared across
        // every /chat request) — disposing the chain after one request would dispose that singleton
        // and break every subsequent call. The wrappers own no other disposable state, so never
        // disposing them leaks nothing; the per-call streaming enumerator is still disposed below.
        //
        // The probe's model-id list (`new[] { _model }`) is inert on this path: LocalSafetyChatClient
        // consults GetAvailableModelIdsAsync only when ChatOptions.ModelId is set, and this class
        // deliberately omits ModelId (see the ChatOptions note below). The list is still passed so
        // the probe reports an accurate set should a future caller ever set ModelId.
        var chat = new LocalSafetyChatClient(
            _client,
            new LocalSafetyOptions { DefaultMaxOutputTokens = 512 },
            new LazyReadinessProbe(() => true, new[] { _model }))
            .WithDegenerationStop();

        var chatMessages = messages.Select(ToChatMessage).ToList();
        // ChatOptions.ModelId intentionally omitted: the bridge already binds `_model` into the
        // underlying ChatClientAdapter at registration time (ByoProviderRegistration passes
        // `model` to AddIronHive*, which threads it through as ChatClientAdapter's modelId ctor
        // arg). ChatClientAdapter.ConvertToRequest resolves `options?.ModelId ?? _modelId`, so
        // setting it here would just repeat the same value already bound at the bridge.
        var chatOptions = new ChatOptions
        {
            MaxOutputTokens = opts.MaxTokens,
            Temperature = opts.Temperature,
        };

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
                var reason = update.FinishReason?.Value;
                if (string.IsNullOrEmpty(update.Text) && reason is null) continue;
                yield return new GenerationChunk(string.IsNullOrEmpty(update.Text) ? null : update.Text, reason);
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
}
