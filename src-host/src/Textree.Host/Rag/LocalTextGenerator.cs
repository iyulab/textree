using System.Runtime.CompilerServices;
using LMSupply;
using LMSupply.Generator;
using LMSupply.Generator.Abstractions;
using LmChatMessage = LMSupply.Generator.Models.ChatMessage;
using LmChatRole = LMSupply.Generator.Models.ChatRole;
using LmGenerationOptions = LMSupply.Generator.Models.GenerationOptions;

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

        var lmMessages = messages.Select(ToLmMessage).ToList();
        var lmOpts = new LmGenerationOptions
        {
            // Bound generation on BOTH paths. The GGUF/llama-server path honors MaxTokens, but
            // the ONNX path (which we pin via "phi-4-mini") only checks MaxNewTokens and defaults
            // it to int.MaxValue when null — i.e. UNBOUNDED. Without setting MaxNewTokens, a 128-cap
            // request ran 3856 tokens (~30x over) in the GATE bench, burning minutes of CPU even
            // for an attached client. Setting both makes the cap effective regardless of backend.
            MaxTokens = opts.MaxTokens,
            MaxNewTokens = opts.MaxTokens,
            Temperature = opts.Temperature,
        };

        // ct is already passed to GenerateChatAsync so the library can check it between tokens.
        // ThrowIfCancellationRequested guards the case where the library yields a chunk without
        // rechecking ct internally — ensures we stop promptly on client disconnect regardless
        // of the backend's internal cancellation discipline.
        await foreach (var chunk in model.GenerateChatAsync(lmMessages, lmOpts, ct))
        {
            ct.ThrowIfCancellationRequested();
            yield return chunk;
        }
    }

    private static LmChatMessage ToLmMessage(ChatMessage m) =>
        new(ParseRole(m.Role), m.Content);

    private static LmChatRole ParseRole(string? role)
    {
        if (string.IsNullOrWhiteSpace(role))
            throw new ArgumentException("Message role must not be null or empty.", nameof(role));

        return role.ToLowerInvariant() switch
        {
            "system" => LmChatRole.System,
            "assistant" => LmChatRole.Assistant,
            "tool" => LmChatRole.Tool,
            // Unknown non-empty roles (e.g. "function") are treated as User — the caller
            // controls message construction, so non-empty unknown values are a soft mismatch.
            _ => LmChatRole.User,
        };
    }

    public async ValueTask DisposeAsync()
    {
        if (_model is not null)
            await _model.DisposeAsync();
        _loadGate.Dispose();
    }
}
