using LMSupply.Embedder;
using Textree.Host;
using Textree.Host.Rag;
using Xunit;

[Trait("Category", "Integration")]
public class VaultManagerScopeTests
{
    private static async Task<VaultManager> NewManagerAsync()
    {
        var opts = new TextreeHostOptions
        {
            IndexRoot = Path.Combine(Path.GetTempPath(), "textree-test", Guid.NewGuid().ToString("n"))
        };
        // Force CPU execution provider: DirectML crashes on this machine's GPU driver.
        // The product uses ExecutionProvider.Auto (GPU-when-available) at runtime; this
        // test-only override lets the integration tests run in any environment.
        var model = await LocalEmbedder.LoadAsync(
            opts.EmbeddingModel,
            new EmbedderOptions { Provider = LMSupply.ExecutionProvider.Cpu });
        return new VaultManager(new LmSupplyEmbeddingService(model), opts);
    }

    [Fact]
    public async Task SubScopeExcludesRootNotes()
    {
        using var mgr = await NewManagerAsync();
        var vault = Path.GetFullPath("fixtures");
        await mgr.ReindexAsync(vault, default);

        var sub = await mgr.SearchAsync(vault, "payment refund", Path.Combine(vault, "sub"), 10, default);
        Assert.NotEmpty(sub);
        Assert.All(sub, h => Assert.Contains(
            $"{Path.DirectorySeparatorChar}sub{Path.DirectorySeparatorChar}", h.SourcePath));

        var root = await mgr.SearchAsync(vault, "payment refund", vault, 10, default);
        Assert.Contains(root, h => h.SourcePath.Contains("sub-note"));
        // North star: same query, scope only differs -> result set differs (root sees >= sub).
        var rootFiles = root.Select(h => h.SourcePath).Distinct().Count();
        var subFiles = sub.Select(h => h.SourcePath).Distinct().Count();
        Assert.True(rootFiles >= subFiles);
    }

    [Fact]
    public async Task ReindexIsIdempotent_NoDuplicateAccumulation()
    {
        using var mgr = await NewManagerAsync();
        var vault = Path.GetFullPath("fixtures");

        await mgr.ReindexAsync(vault, default);
        var after1 = await mgr.SearchAsync(vault, "payment refund", vault, 50, default);

        await mgr.ReindexAsync(vault, default);   // reindex the SAME vault again
        var after2 = await mgr.SearchAsync(vault, "payment refund", vault, 50, default);

        // Re-indexing the same unchanged content must not grow the index (idempotent upsert).
        Assert.Equal(after1.Count, after2.Count);
    }
}
