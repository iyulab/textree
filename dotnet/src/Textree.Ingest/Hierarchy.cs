using System.Text;
using System.Text.Json.Nodes;

namespace Textree.Ingest;

/// <summary>A heading of a document: level, text, and where its line is (UTF-8 byte offsets).</summary>
public sealed record DocHeading(int Level, string Text, int Start, int End);

/// <summary>A document as ingest sees it.</summary>
public sealed record Doc(string Path, string Text, IReadOnlyList<DocHeading> Headings)
{
    public string Folder => Hierarchy.DirOf(Path);
    public string Name => Hierarchy.NameOf(Path);

    /// <summary>The first top-level heading, else the file name without <c>.md</c>.</summary>
    public string Title =>
        Headings.FirstOrDefault(h => h.Level == 1)?.Text
        ?? (Name.EndsWith(".md", StringComparison.OrdinalIgnoreCase) ? Name[..^3] : Name);

    /// <summary>The body of the section under <paramref name="heading"/>, or null.</summary>
    public string? SectionBody(DocHeading heading)
    {
        var bytes = Encoding.UTF8.GetBytes(Text);
        var i = Headings.ToList().IndexOf(heading);
        if (i < 0)
        {
            return null;
        }
        var lineEnd = Array.IndexOf(bytes, (byte)'\n', Math.Min(heading.End, bytes.Length));
        var start = lineEnd < 0 ? bytes.Length : lineEnd + 1;
        var next = Headings.Skip(i + 1).FirstOrDefault(h => h.Level <= heading.Level);
        var end = Math.Max(start, next?.Start ?? bytes.Length);
        return Encoding.UTF8.GetString(bytes, start, end - start).Trim('\n');
    }
}

/// <summary>The folder hierarchy read once for an ingest: folders and documents, in memory.</summary>
public sealed class Hierarchy
{
    private readonly Dictionary<string, Doc> _docs;
    private readonly SortedSet<string> _folders;

    private Hierarchy(Dictionary<string, Doc> docs)
    {
        _docs = docs;
        _folders = new SortedSet<string>(StringComparer.Ordinal) { "" };
        foreach (var d in docs.Values)
        {
            for (var dir = d.Folder; dir.Length > 0; dir = DirOf(dir))
            {
                _folders.Add(dir);
            }
        }
    }

    public IReadOnlyCollection<Doc> Documents => _docs.Values;

    public Doc? Document(string path) => _docs.GetValueOrDefault(path);

    public bool IsFolder(string path) => _folders.Contains(path);

    /// <summary>Reads a folder through the engine.</summary>
    public static Hierarchy Read(Folder folder)
    {
        var result = folder.Documents();
        if (result.Error is { } error)
        {
            throw new InvalidOperationException(error.Message);
        }
        var docs = new Dictionary<string, Doc>(StringComparer.Ordinal);
        foreach (var node in result.Value!["documents"]!.AsArray())
        {
            var path = node!["path"]!.GetValue<string>();
            var headings = node["headings"]?.AsArray()
                .Select(h => new DocHeading(
                    h!["level"]!.GetValue<int>(),
                    h["text"]!.GetValue<string>(),
                    h["range"]!["start"]!.GetValue<int>(),
                    h["range"]!["end"]!.GetValue<int>()))
                .ToList() ?? [];
            docs[path] = new Doc(path, node["text"]!.GetValue<string>(), headings);
        }
        return new Hierarchy(docs);
    }

    /// <summary>The folders directly in <paramref name="folder"/>.</summary>
    public IReadOnlyList<string> SubfoldersOf(string folder) =>
        _folders.Where(f => f.Length > 0 && DirOf(f) == folder).ToList();

    /// <summary>The documents directly in <paramref name="folder"/>.</summary>
    public IReadOnlyList<Doc> DocumentsIn(string folder) =>
        _docs.Values.Where(d => d.Folder == folder).OrderBy(d => d.Path, StringComparer.Ordinal).ToList();

    /// <summary>All documents under <paramref name="folder"/>, nearest first.</summary>
    public IEnumerable<Doc> DocumentsUnder(string folder) =>
        _docs.Values
            .Where(d => folder.Length == 0 || d.Path.StartsWith(folder + "/", StringComparison.Ordinal))
            .OrderBy(d => d.Path.Count(c => c == '/'))
            .ThenBy(d => d.Path, StringComparer.Ordinal);

    /// <summary>The folder's own document (<c>name/name.md</c>), if it has one.</summary>
    public Doc? FolderDocument(string folder) =>
        folder.Length == 0 ? null : Document($"{folder}/{NameOf(folder)}.md");

    /// <summary>The keys of every paragraph in the folder.</summary>
    public HashSet<string> ParagraphKeys()
    {
        var keys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var d in _docs.Values)
        {
            keys.UnionWith(Paragraphs.Keys(d.Text));
        }
        return keys;
    }

    /// <summary>A version of the part of the hierarchy under <paramref name="folder"/>: it changes when anything under it changes.</summary>
    public string VersionOf(string folder)
    {
        var sb = new StringBuilder();
        foreach (var d in DocumentsUnder(folder).OrderBy(d => d.Path, StringComparer.Ordinal))
        {
            sb.Append(d.Path).Append('\0').Append(d.Text.Length).Append('\0').Append(d.Title).Append('\n');
        }
        return Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(sb.ToString())), 0, 12);
    }

    public static string DirOf(string path)
    {
        var i = path.LastIndexOf('/');
        return i < 0 ? "" : path[..i];
    }

    public static string NameOf(string path)
    {
        var i = path.LastIndexOf('/');
        return i < 0 ? path : path[(i + 1)..];
    }

    public static string Join(string folder, string name) => folder.Length == 0 ? name : $"{folder}/{name}";
}
