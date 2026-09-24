namespace Textree.Host.Rag;

/// <summary>
/// Detects pathological repetition ("degeneration") in streamed generation output — the failure
/// mode where a small model loops the same word or short phrase (e.g. "concisely concisely
/// concisely…") until it hits the token cap. Pure and allocation-light so the streaming loop can
/// call it once per chunk.
///
/// Only the tail is inspected (degeneration always manifests at the end of the stream), and a
/// match requires the repeating unit to contain a letter — so legitimate Markdown structure
/// (table separators "| --- |", horizontal rules "----", punctuation runs "....", "===") never
/// trips it. The bar is deliberately high (a short unit repeated at least four times in a row) so
/// ordinary prose, lists, and repeated function words stay well clear; the guard errs toward NOT
/// firing to keep any trade-off off good answers.
/// </summary>
/// <remarks>
/// TODO(upstream: iron-prow — a built-in degeneration guard for its guard slot; the default is a
/// no-op, so this check and the two tail loops that call it live here until one ships).
/// </remarks>
public static class RepetitionGuard
{
    private const int Window = 240;     // chars of tail to inspect
    private const int MinRepeats = 4;   // a unit must repeat at least this many times to count
    private const int MaxPeriod = 60;   // longest repeating unit we look for

    /// <summary>True when the tail of <paramref name="text"/> is a short unit repeated many times.</summary>
    public static bool IsDegenerate(string text)
    {
        if (string.IsNullOrEmpty(text) || text.Length < MinRepeats * 2) return false;

        int len = text.Length;
        int windowLen = len > Window ? Window : len;
        int maxPeriod = System.Math.Min(MaxPeriod, windowLen / MinRepeats);

        for (int p = 1; p <= maxPeriod; p++)
        {
            if (!PeriodicSuffix(text, len, p, MinRepeats)) continue;
            // Require a letter in the repeating unit — excludes "----", "| --- |", "....", "===".
            if (HasLetter(text, len - p, p)) return true;
        }
        return false;
    }

    // True when the last (period * repeats) chars ending at `end` are `period`-periodic.
    private static bool PeriodicSuffix(string text, int end, int period, int repeats)
    {
        int needed = period * repeats;
        if (end < needed) return false;
        int start = end - needed;
        for (int i = start + period; i < end; i++)
        {
            if (text[i] != text[i - period]) return false;
        }
        return true;
    }

    private static bool HasLetter(string text, int start, int count)
    {
        for (int i = start; i < start + count; i++)
        {
            if (char.IsLetter(text[i])) return true;
        }
        return false;
    }
}
