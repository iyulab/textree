using Textree.Host.Rag;
using Xunit;

public class EmbedderLazyTests
{
    [Fact]
    public void Not_ready_before_the_model_loads()
    {
        var slot = new EmbedderSlot();
        Assert.False(slot.Ready);
    }

    [Fact]
    public void Asking_for_the_service_before_the_model_loads_says_so()
    {
        var slot = new EmbedderSlot();
        var ex = Assert.Throws<InvalidOperationException>(() => slot.Service);
        Assert.Contains("not loaded", ex.Message);
    }
}
