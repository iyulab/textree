using System.Diagnostics;
using Textree.Host.Rag;
using Xunit;
using Xunit.Abstractions;

// Step 8 GATE measurement: load the REAL local generator (Phi-4 Mini ONNX, CPU) and measure
// tokens/sec on a representative prompt (~1KB context + a question). This is a feasibility
// probe, not a correctness test — it depends on the model being present on disk, so it is
// marked Integration and excluded from the model-independent default run. Run explicitly:
//   dotnet test src-host/TextreeHost.slnx --filter TextGeneratorBench
[Trait("Category", "Integration")]
public sealed class TextGeneratorBenchTests
{
    private readonly ITestOutputHelper _out;

    public TextGeneratorBenchTests(ITestOutputHelper @out) => _out = @out;

    // Skipped by default: this is a manual feasibility probe that loads a multi-GB model and
    // takes minutes — it must not run on the normal verification gate. Remove Skip (or run with
    // an explicit filter that overrides Skip) to take a fresh measurement.
    [Fact(Skip = "Manual GATE measurement; loads a multi-GB model. Run deliberately, not on CI.")]
    public async Task Measure_tokens_per_second_on_representative_prompt()
    {
        // ~1KB of context, then a question — representative of a /ask grounded on a note.
        const string context =
            "Textree is a filesystem-based single-user Markdown note app. Its core values are " +
            "data sovereignty (the truth is the standard .md file, no lock-in), the filesystem " +
            "as the source of truth (.textree/ is a regenerable cache), minimalism (simplicity " +
            "beats expressiveness), local AI-first with graceful degradation (editing, tree and " +
            "search work fully even without AI), and being permanently free and open source. " +
            "The .textree sidecar holds order.json, favorites.json and views; on conflict the " +
            "Markdown file always wins. Writes are atomic (temp then fsync then rename); deletes " +
            "go to .textree/trash. The Rust backend validates every path at the IPC boundary with " +
            "is_within, is_valid_name and Component::Normal. Full-text search uses a local tantivy " +
            "ngram index with no database. The local AI host borrows RAG, embedding and runtime " +
            "substrate rather than reinventing it, and pins the CPU execution provider because " +
            "DirectML crashes during inference on the target hardware and does not fall back.";
        const string question = "In one sentence, what is the source of truth in Textree and why?";

        await using var gen = new LocalTextGenerator(new ModelStatus());

        var loadSw = Stopwatch.StartNew();
        await gen.PrepareAsync(CancellationToken.None);
        loadSw.Stop();
        Assert.True(gen.Ready);

        var messages = new List<ChatMessage>
        {
            new("system", "You are a concise assistant. Answer using only the provided context."),
            new("user", context + "\n\n" + question),
        };
        var opts = new GenerationOptions(MaxTokens: 128, Temperature: 0.2f);

        var sb = new System.Text.StringBuilder();
        var chunkCount = 0;
        var genSw = Stopwatch.StartNew();
        await foreach (var chunk in gen.GenerateAsync(messages, opts, CancellationToken.None))
        {
            sb.Append(chunk);
            chunkCount++;
        }
        genSw.Stop();

        // Chunk count is a lower bound on emitted tokens (each chunk is >= 1 token). Report it
        // as the tokens/sec proxy; the raw seconds and full text are logged for the controller.
        var seconds = genSw.Elapsed.TotalSeconds;
        var chunksPerSec = seconds > 0 ? chunkCount / seconds : 0;

        _out.WriteLine($"[GATE] model=phi-4-mini (Phi-4 Mini 3.8B, ONNX CPU-int4)");
        _out.WriteLine($"[GATE] load time     : {loadSw.Elapsed.TotalSeconds:F2} s");
        _out.WriteLine($"[GATE] generation    : {chunkCount} chunks in {seconds:F2} s");
        _out.WriteLine($"[GATE] throughput    : {chunksPerSec:F2} chunks(tokens)/s");
        _out.WriteLine($"[GATE] output        : {sb}");

        // After the 0.35.1 + iron-prow bridge migration, bounding no longer depends on
        // LocalTextGenerator setting LmGenerationOptions.MaxNewTokens by hand: LMSupply 0.35.1's
        // ResolveMaxOutputTokens (MaxNewTokens ?? MaxTokens) closes the ONNX "ignore MaxTokens" gap,
        // and the bridge maps ChatOptions.MaxOutputTokens -> that path. This test is the manual proof
        // that the cap still holds WITHOUT the removed dual-set hack.
        Assert.True(chunkCount > 0, "generator produced no tokens");
        // Each chunk is >= 1 token, so chunkCount is a lower bound on emitted tokens. With a 128-token
        // cap, a correct bound keeps chunkCount at or below the cap plus a small tolerance for chunking
        // (a regression to the old unbounded behavior ran ~3856). Allow generous headroom to avoid
        // tokenizer-boundary flakiness while still catching a 30x blowout.
        Assert.True(chunkCount <= 256, $"generation exceeded the bound (chunks={chunkCount}); MaxTokens cap not honored");
    }
}
