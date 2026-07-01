using System.Runtime.CompilerServices;
using IronProw.LMSupply;
using LMSupply;
using LMSupply.Generator;
using LMSupply.Generator.Abstractions;
using Microsoft.Extensions.AI;
// The current namespace (Textree.Host.Rag) declares its own ChatMessage record (see
// ITextGenerator.cs), which takes precedence over the M.E.AI ChatMessage brought in by the
// `using Microsoft.Extensions.AI;` above for any unqualified `ChatMessage` reference in this
// file. Alias the M.E.AI type explicitly so ToChatMessage's return type/construction are
// unambiguous.
using MeaiChatMessage = Microsoft.Extensions.AI.ChatMessage;

namespace Textree.Host.Rag;

// ── Generator backend decision (Task 1, P3 /ask) ───────────────────────────────
// Path taken: DOGFOOD — LMSupply.Generator 0.34.28 (LocalGenerator.LoadAsync +
// IGeneratorModel.GenerateChatAsync), the streaming text-generation sibling of the
// existing LMSupply.Embedder we already ship. Chosen directly over the brief's
// suggested FluxIndex.Providers.LMSupply.LMSupplyTextCompletionService because:
//   1. It mirrors the embedder precedent exactly — LocalEmbedder.LoadAsync +
//      EmbedderOptions{Provider=Cpu} ↔ LocalGenerator.LoadAsync + GeneratorOptions{Provider=Cpu}.
//   2. EmbeddingService.cs already documents that the FluxIndex.Providers.LMSupply
//      adapter had diverged signatures, so they kept the hand-rolled LMSupply.* adapter;
//      routing /chat through FluxIndex would fight that existing precedent.
//   3. LMSupply.Generator exposes a first-class IAsyncEnumerable<string> streaming chat
//      API (GenerateChatAsync) that maps 1:1 to the SSE token stream we need — no extra
//      RAG/completion-service indirection.
// Execution provider is pinned to CPU: DirectML (the default Auto provider) crashes on
// *inference* on this hardware ("LayerNormalization DmlExecutionProvider 0x80070057") and
// does NOT auto-fall-back to CPU. Same lesson the embedder startup already encodes.
// Default model: the LMSupply "phi-4-mini" alias (Microsoft Phi-4 Mini, 3.8B, MIT,
// multilingual, ONNX CPU-int4). We pin this alias rather than "default" on purpose:
// "default" does hardware-aware auto-selection that, on a CPU machine, routes to a Gemma-4
// GGUF served via an out-of-process llama-server — a heavier, less deterministic backend.
// "phi-4-mini" pins the in-process ONNX Runtime GenAI path that matches our CPU-pinned,
// self-contained host model. This is NOT a locked production choice — model selection is a
// later decision; Step 8 only measures feasibility.

/// <summary>
/// Local CPU text generator backed by LMSupply.Generator. Lazily loads the model on first
/// use; streams token chunks honoring the caller's <see cref="CancellationToken"/> so a
/// client disconnect stops generation and frees CPU.
/// </summary>
public sealed class LocalTextGenerator : ITextGenerator, IAsyncDisposable
{
    // "phi-4-mini" = Microsoft Phi-4 Mini (MIT, ONNX). LMSupply resolves preset aliases the
    // same way the embedder resolves "fast"/"default"/"quality". See header for why this is
    // pinned instead of "default" (which auto-selects a GGUF/llama-server backend on CPU).
    private const string ModelId = "phi-4-mini";

    private readonly ModelStatus _status;
    private readonly SemaphoreSlim _loadGate = new(1, 1);
    // Guarded by _loadGate for writes; read lock-free via Volatile.Read on the fast path.
    private IGeneratorModel? _model;
    // Mirrors _model's Volatile read/write discipline: written inside _loadGate, read lock-free.
    private string? _lastError;

    public LocalTextGenerator(ModelStatus status) => _status = status;

    public bool Ready => Volatile.Read(ref _model) is not null;
    public string? LastError => Volatile.Read(ref _lastError);

    /// <summary>Idempotent: loads the model once; subsequent calls are no-ops.</summary>
    public async Task PrepareAsync(CancellationToken ct)
    {
        if (Volatile.Read(ref _model) is not null)
            return;

        await _loadGate.WaitAsync(ct);
        try
        {
            if (_model is not null)
                return;

            // Fresh attempt: clear any error from a prior failed attempt so a successful (or
            // still-in-progress) retry is not reported as failed.
            Volatile.Write(ref _lastError, null);
            try
            {
                // Signal download start before invoking the library so /health reflects the
                // actual phase even if download progress callbacks never fire (cached model).
                _status.SetGeneratorPhase(ModelPhase.Downloading);

                // Pin CPU: DirectML crashes on inference here and does not fall back. See header.
                var model = await LocalGenerator.LoadAsync(
                    ModelId,
                    new GeneratorOptions { Provider = ExecutionProvider.Cpu },
                    progress: _status.GeneratorProgress,
                    cancellationToken: ct);

                _status.SetGeneratorPhase(ModelPhase.Ready);
                Volatile.Write(ref _model, model);
            }
            catch (OperationCanceledException)
            {
                // Cancellation is not a load failure — do not record it as a generator error.
                throw;
            }
            catch (Exception ex)
            {
                Volatile.Write(ref _lastError, ex.Message);
                _status.SetGeneratorError(ex.Message);
                throw;
            }
        }
        finally
        {
            _loadGate.Release();
        }
    }

    public async IAsyncEnumerable<string> GenerateAsync(
        IReadOnlyList<ChatMessage> messages,
        GenerationOptions opts,
        [EnumeratorCancellation] CancellationToken ct)
    {
        await PrepareAsync(ct);
        var model = Volatile.Read(ref _model)!;

        // Borrow the iron-prow D12 bridge: the lightweight 2-decorator path (NOT AddLMSupplyLocal,
        // whose selection/registry/resilience gateway is inert for a single local provider).
        // GeneratorChatClient wraps our already-loaded IGeneratorModel (IGeneratorModel : ITextGenerator,
        // compile-proven) as an IChatClient; LocalSafetyChatClient adds length-bounding + a readiness gate.
        using var chat = new LocalSafetyChatClient(
            new GeneratorChatClient(model),
            new LocalSafetyOptions { DefaultMaxOutputTokens = 512 }, // belt-and-suspenders; we always set a cap below
            new LazyReadinessProbe(() => Ready, new[] { ModelId }));

        var chatMessages = messages.Select(ToChatMessage).ToList();
        var chatOptions = new ChatOptions
        {
            // 0.35.1 ResolveMaxOutputTokens (MaxNewTokens ?? MaxTokens) makes a single mapping enough;
            // the old dual-set (MaxTokens + MaxNewTokens) hack is no longer needed.
            MaxOutputTokens = opts.MaxTokens,
            Temperature = opts.Temperature,
        };

        // ct is threaded into GetStreamingResponseAsync so the library can observe it between tokens.
        // ThrowIfCancellationRequested keeps our belt-and-suspenders guard: stop promptly on client
        // disconnect regardless of the bridge's internal cancellation discipline (free CPU).
        await foreach (var update in chat.GetStreamingResponseAsync(chatMessages, chatOptions, ct))
        {
            ct.ThrowIfCancellationRequested();
            if (!string.IsNullOrEmpty(update.Text))
                yield return update.Text;
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
            // Unknown non-empty roles are treated as User — the caller controls message construction.
            _ => ChatRole.User,
        };
    }

    public async ValueTask DisposeAsync()
    {
        if (_model is not null)
            await _model.DisposeAsync();
        _loadGate.Dispose();
    }
}
