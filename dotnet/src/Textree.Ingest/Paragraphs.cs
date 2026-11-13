using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace Textree.Ingest;

/// <summary>
/// Paragraphs as ingest compares them: blocks of text separated by blank lines, with runs of
/// white space made one space, Unicode in composed form (NFC), and a heading line reduced to its
/// text (so <c># Watering</c> and <c>## Watering</c> are the same paragraph). Text inside fenced
/// code blocks is one paragraph per block.
/// </summary>
public static partial class Paragraphs
{
    /// <summary>The paragraphs of a text, in order, as written (trimmed).</summary>
    public static IReadOnlyList<string> Split(string text)
    {
        var result = new List<string>();
        var current = new StringBuilder();
        var inFence = false;
        void Flush()
        {
            var block = current.ToString().Trim('\n', '\r');
            if (block.Trim().Length > 0)
            {
                result.Add(block.TrimEnd());
            }
            current.Clear();
        }
        foreach (var raw in text.Replace("\r\n", "\n").Split('\n'))
        {
            var line = raw;
            var trimmed = line.TrimStart();
            if (trimmed.StartsWith("```") || trimmed.StartsWith("~~~"))
            {
                if (!inFence)
                {
                    Flush();
                    inFence = true;
                    current.Append(line).Append('\n');
                    continue;
                }
                current.Append(line).Append('\n');
                inFence = false;
                Flush();
                continue;
            }
            if (inFence)
            {
                current.Append(line).Append('\n');
                continue;
            }
            if (line.Trim().Length == 0)
            {
                Flush();
                continue;
            }
            // A heading line is a paragraph of its own.
            if (HeadingLine().IsMatch(line))
            {
                Flush();
                current.Append(line);
                Flush();
                continue;
            }
            current.Append(line).Append('\n');
        }
        Flush();
        return result;
    }

    /// <summary>The comparable form of one paragraph.</summary>
    public static string Normalize(string paragraph)
    {
        var p = paragraph.Normalize(NormalizationForm.FormC).Trim();
        var heading = HeadingLine().Match(p);
        if (heading.Success && !p.Contains('\n'))
        {
            p = heading.Groups["text"].Value.TrimEnd('#', ' ');
        }
        return Whitespace().Replace(p, " ").Trim();
    }

    /// <summary>A short stable key of a paragraph's comparable form.</summary>
    public static string Key(string paragraph)
    {
        var bytes = SHA256.HashData(Encoding.UTF8.GetBytes(Normalize(paragraph)));
        return Convert.ToHexString(bytes, 0, 12);
    }

    /// <summary>The keys of every paragraph of a text.</summary>
    public static HashSet<string> Keys(string text) => Split(text).Select(Key).ToHashSet();

    /// <summary>Whether a paragraph is a heading line, and its level.</summary>
    public static int HeadingLevel(string paragraph)
    {
        var m = HeadingLine().Match(paragraph.Trim());
        return m.Success && !paragraph.Trim().Contains('\n') ? m.Groups["marks"].Value.Length : 0;
    }

    [GeneratedRegex(@"^ {0,3}(?<marks>#{1,6})[ \t]+(?<text>.*)$")]
    private static partial Regex HeadingLine();

    [GeneratedRegex(@"\s+")]
    private static partial Regex Whitespace();
}
