using System.Net;
using System.Net.Http.Json;
using Textree.Host.Rag;
using Xunit;

// Error-boundary tests for POST /index. A vault is a live directory tree: a change notification
// can name a file that is already gone by the time indexing runs. That race is a normal condition
// of watching a filesystem, not a server fault, so it must not surface as an unhandled failure.
public sealed class IndexEndpointTests
{
    [Fact]
    public async Task Index_reports_gone_when_source_no_longer_exists()
    {
        var vault = Directory.CreateTempSubdirectory("index-endpoint-tests-").FullName;
        try
        {
            using var factory = new ChatEndpointTests.Factory(new NoopGenerator());
            var client = factory.CreateClient();

            var resp = await client.PostAsJsonAsync("/index",
                new { vaultPath = vault, path = Path.Combine(vault, "vanished.md") });

            Assert.Equal(HttpStatusCode.NotFound, resp.StatusCode);
            Assert.NotEqual(HttpStatusCode.InternalServerError, resp.StatusCode);
        }
        finally
        {
            Directory.Delete(vault, recursive: true);
        }
    }

    [Fact]
    public async Task Index_rejects_blank_paths_at_the_edge()
    {
        using var factory = new ChatEndpointTests.Factory(new NoopGenerator());
        var client = factory.CreateClient();

        var resp = await client.PostAsJsonAsync("/index", new { vaultPath = "", path = "" });

        Assert.Equal(HttpStatusCode.BadRequest, resp.StatusCode);
    }

    private sealed class NoopGenerator : ITextGenerator
    {
        public bool Ready => true;
        public string? LastError => null;
        public Task PrepareAsync(CancellationToken ct) => Task.CompletedTask;

        public async IAsyncEnumerable<GenerationChunk> GenerateAsync(
            IReadOnlyList<ChatMessage> messages,
            GenerationOptions options,
            [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken ct = default)
        {
            await Task.CompletedTask;
            yield break;
        }
    }
}
