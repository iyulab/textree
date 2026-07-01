using System.Linq;
using Textree.Host.Rag;
using Xunit;

// Unit tests for RepetitionGuard.IsDegenerate — the pure detector that stops a small model's
// pathological repetition loop. Covers the real reported failure ("concisely concisely…"), phrase
// loops, and — crucially — the false-positive guards: Markdown structure and punctuation runs
// must NOT be flagged, since the guard errs toward keeping trade-offs off good answers.
public sealed class RepetitionGuardTests
{
    private static string Repeat(string unit, int times) => string.Concat(Enumerable.Repeat(unit, times));

    [Fact]
    public void Flags_the_reported_single_word_loop()
    {
        // The exact degeneration the user hit: one word repeated until the token cap.
        Assert.True(RepetitionGuard.IsDegenerate(Repeat("concisely ", 40)));
    }

    [Fact]
    public void Flags_a_repeated_short_phrase()
    {
        Assert.True(RepetitionGuard.IsDegenerate("intro. " + Repeat("the cat sat ", 6)));
    }

    [Fact]
    public void Flags_a_repeated_function_word()
    {
        Assert.True(RepetitionGuard.IsDegenerate("... and " + Repeat("very ", 12)));
    }

    [Fact]
    public void Flags_a_cjk_word_loop()
    {
        // char.IsLetter is true for CJK, so a CJK loop is caught the same way.
        Assert.True(RepetitionGuard.IsDegenerate(Repeat("네 ", 20)));
    }

    [Fact]
    public void Ignores_ordinary_prose()
    {
        const string prose =
            "Photosynthesis is the process by which plants convert sunlight into energy. "
            + "Chlorophyll in the leaves absorbs light and drives the conversion of carbon "
            + "dioxide and water into glucose, releasing oxygen as a by-product.";
        Assert.False(RepetitionGuard.IsDegenerate(prose));
    }

    [Fact]
    public void Ignores_a_markdown_table_separator()
    {
        // No letter in the repeating unit → must not trip, even across many columns.
        Assert.False(RepetitionGuard.IsDegenerate("| " + Repeat("--- | ", 8)));
    }

    [Fact]
    public void Ignores_a_markdown_list()
    {
        const string list = "- Compost improves soil\n- Water deeply once a week\n- Mulch retains moisture\n";
        Assert.False(RepetitionGuard.IsDegenerate(list));
    }

    [Theory]
    [InlineData("--------------------")]     // horizontal rule
    [InlineData("====================")]     // setext underline
    [InlineData("!!!!!!!!!!!!!!!!!!!!")]     // punctuation run
    [InlineData("....................")]     // ellipsis run
    [InlineData("")]                          // empty
    [InlineData("hi")]                        // too short to judge
    public void Ignores_non_letter_runs_and_short_text(string text)
    {
        Assert.False(RepetitionGuard.IsDegenerate(text));
    }
}
