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
        // CPU here on purpose: results must not depend on which GPU the machine running the
        // tests has. The product lets the runtime pick (ExecutionProvider.Auto).
        var model = await LocalEmbedder.LoadAsync(
            opts.EmbeddingModel,
            new EmbedderOptions { Provider = LMSupply.ExecutionProvider.Cpu });
        var slot = new EmbedderSlot();
        slot.Set(model);
        return new VaultManager(slot, opts);
    }

    [Fact]
    public async Task SubScopeExcludesRootNotes()
    {
        using var mgr = await NewManagerAsync();
        var vault = Path.GetFullPath("fixtures");
        await mgr.ReindexAsync(vault, default);

        var sub = await mgr.SearchAsync(vault, "payment refund", Path.Combine(vault, "sub"), 10, default);
        Assert.NotEmpty(sub);
        // SearchAsync returns vault-relative POSIX paths (e.g. "sub/sub-note.md"), so the
        // separator is always '/' regardless of host OS. Assert on POSIX sub-path prefix.
        Assert.All(sub, h => Assert.StartsWith("sub/", h.SourcePath));

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
