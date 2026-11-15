using Textree.Eval;
using Textree.Ingest;
using Xunit;

namespace Textree.Eval.Tests;

public sealed class RebuildTests
{
    [Fact]
    public void A_document_belongs_to_the_folder_holding_most_of_its_paragraphs_at_the_end()
    {
        var root = Path.Combine(Path.GetTempPath(), "textree-rebuild-test-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(root, "zgarden"));
        Directory.CreateDirectory(Path.Combine(root, "reading"));
        try
        {
            // "watering" was split: two of its paragraphs went to zgarden/, one to reading/ — the majority wins over ordinal order.
            File.WriteAllText(Path.Combine(root, "zgarden", "beds.md"), "# Beds\n\nWater at dawn.\n\nMulch keeps it in.\n");
            File.WriteAllText(Path.Combine(root, "reading", "books.md"), "# Books\n\nA book on soil.\n");
            var sources = new List<Doc>
            {
                new("src/watering.md", "# Watering\n\nWater at dawn.\n\nMulch keeps it in.\n\nA book on soil.\n", []),
                new("src/lost.md", "Nothing of this was kept.\n", []),
            };
            var final = Rebuild.FinalFolders(sources, Hierarchy.Read(new Folder(root)));
            Assert.Equal("zgarden", final["src/watering.md"]);
            Assert.False(final.ContainsKey("src/lost.md"));
        }
        finally
        {
            Directory.Delete(root, true);
        }
    }
}
