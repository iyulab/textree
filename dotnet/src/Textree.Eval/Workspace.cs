using System.Security.Cryptography;
using System.Text;

namespace Textree.Eval;

/// <summary>A copy of a folder in the temporary directory, removed afterwards. The original is only read.</summary>
public sealed class Workspace : IDisposable
{
    public Workspace(string source)
    {
        Root = Path.Combine(Path.GetTempPath(), "textree-eval-" + Guid.NewGuid().ToString("N")[..12]);
        Copy(source, Root);
    }

    public string Root { get; }

    public static void Copy(string from, string to)
    {
        Directory.CreateDirectory(to);
        foreach (var dir in Directory.EnumerateDirectories(from, "*", SearchOption.AllDirectories))
        {
            Directory.CreateDirectory(Path.Combine(to, Path.GetRelativePath(from, dir)));
        }
        foreach (var file in Directory.EnumerateFiles(from, "*", SearchOption.AllDirectories))
        {
            File.Copy(file, Path.Combine(to, Path.GetRelativePath(from, file)));
        }
    }

    /// <summary>Renames every file and folder under <paramref name="root"/> to Unicode normalization form D.</summary>
    public static void StoreNfd(string root)
    {
        foreach (var entry in Directory.EnumerateFileSystemEntries(root, "*", SearchOption.AllDirectories)
                     .OrderByDescending(e => e.Count(c => c == Path.DirectorySeparatorChar)).ToList())
        {
            var name = Path.GetFileName(entry);
            var nfd = name.Normalize(NormalizationForm.FormD);
            if (nfd == name)
            {
                continue;
            }
            var dir = Path.GetDirectoryName(entry)!;
            var temp = Path.Combine(dir, "~nfd-" + Guid.NewGuid().ToString("N")[..8]);
            if (Directory.Exists(entry))
            {
                Directory.Move(entry, temp);
                Directory.Move(temp, Path.Combine(dir, nfd));
            }
            else
            {
                File.Move(entry, temp);
                File.Move(temp, Path.Combine(dir, nfd));
            }
        }
    }

    /// <summary>Every file (relative path, '/'-separated) and its bytes.</summary>
    public static SortedDictionary<string, byte[]> Files(string root)
    {
        var files = new SortedDictionary<string, byte[]>(StringComparer.Ordinal);
        foreach (var f in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories))
        {
            files[Path.GetRelativePath(root, f).Replace('\\', '/')] = File.ReadAllBytes(f);
        }
        return files;
    }

    /// <summary>A fingerprint of a folder: file list, bytes and modification times.</summary>
    public static string Fingerprint(string root)
    {
        var sb = new StringBuilder();
        foreach (var f in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories).Order(StringComparer.Ordinal))
        {
            sb.Append(Path.GetRelativePath(root, f)).Append('\0')
              .Append(Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(f)))).Append('\0')
              .Append(File.GetLastWriteTimeUtc(f).Ticks).Append('\n');
        }
        return Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(sb.ToString())));
    }

    public void Dispose()
    {
        try { Directory.Delete(Root, recursive: true); } catch (IOException) { }
    }
}
