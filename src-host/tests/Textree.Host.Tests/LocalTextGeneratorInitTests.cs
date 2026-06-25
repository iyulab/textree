using Textree.Host.Rag;
using Xunit;

// Lightweight initial-state tests for LocalTextGenerator. These do not load a real model —
// they only verify the constructor wires the ModelStatus dependency and the pre-load
// invariants hold (Ready=false, LastError=null, generator phase=Idle).
public sealed class LocalTextGeneratorInitTests
{
    [Fact]
    public async Task Constructor_accepts_ModelStatus_and_Ready_is_false()
    {
        await using var gen = new LocalTextGenerator(new ModelStatus());
        Assert.False(gen.Ready);
    }

    [Fact]
    public async Task LastError_is_null_before_any_load_attempt()
    {
        await using var gen = new LocalTextGenerator(new ModelStatus());
        Assert.Null(gen.LastError);
    }

    [Fact]
    public async Task Generator_snapshot_starts_Idle()
    {
        var status = new ModelStatus();
        await using var gen = new LocalTextGenerator(status);
        Assert.Equal(ModelPhase.Idle, status.Generator.Phase);
    }
}
