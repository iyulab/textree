using Textree.Eval;
using Xunit;

namespace Textree.Eval.Tests;

public class LabelsTests
{
    [Fact]
    public void A_note_splits_into_one_claim_per_sentence_or_list_line()
    {
        var claims = Labels.Claims("# Drip line\n\nLaid two rows. It uses half the water!\n- Run it every two days.\n\n#garden #water\n");
        Assert.Equal(["Laid two rows.", "It uses half the water!", "Run it every two days."], claims);
    }
}
