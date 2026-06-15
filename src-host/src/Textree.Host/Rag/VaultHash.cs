using System.Security.Cryptography;
using System.Text;

namespace Textree.Host.Rag;

public static class VaultHash
{
    public static string For(string vaultPath)
    {
        var canonical = Path.GetFullPath(vaultPath)
            .TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)
            .Replace('\\', '/')
            .ToLowerInvariant();
        var bytes = SHA256.HashData(Encoding.UTF8.GetBytes(canonical));
        return Convert.ToHexString(bytes)[..16].ToLowerInvariant();
    }
}
