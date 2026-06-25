using Textree.Host.Rag;
using Xunit;

public class EmbedderLazyTests
{
    [Fact]
    public void Dimensions_is_zero_before_model_set()
    {
        var svc = new LmSupplyEmbeddingService();
        Assert.Equal(0, svc.Dimensions);
        Assert.Equal(0, svc.GetEmbeddingDimension());
    }

    [Fact]
    public async Task EmbedCore_throws_before_model_set()
    {
        var svc = new LmSupplyEmbeddingService();
        await Assert.ThrowsAsync<InvalidOperationException>(
            () => svc.GenerateEmbeddingsBatchAsync(new[] { "hi" }));
    }
}
