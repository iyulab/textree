namespace Textree.Host;

public sealed class TextreeHostOptions
{
    // "fast" = multilingual-e5-small (384d, ~470MB). Confirmed LMSupply preset alias
    // (Filer FilerRagOptions: "fast"=e5-small/384d, "default"=e5-base/768d, "quality"=BGE-M3/1024d).
    public string EmbeddingModel { get; set; } = "fast";

    public string IndexRoot { get; set; } =
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                     "textree", "index");
}
