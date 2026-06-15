using Textree.Host.Rag;
using Xunit;

public class VaultHashTests
{
    [Fact]
    public void SamePathNormalizedToSameHash()
    {
        var a = VaultHash.For(@"C:\data\vault");
        var b = VaultHash.For(@"C:\data\vault\");      // trailing separator
        var c = VaultHash.For(@"C:/data/VAULT");        // separators + casing
        Assert.Equal(a, b);
        Assert.Equal(a, c);
    }

    [Fact]
    public void DifferentPathsDifferHash()
        => Assert.NotEqual(VaultHash.For(@"C:\data\v1"), VaultHash.For(@"C:\data\v2"));

    [Fact]
    public void HashIs16HexChars()
        => Assert.Matches("^[0-9a-f]{16}$", VaultHash.For(@"C:\x"));
}
