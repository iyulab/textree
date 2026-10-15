namespace Textree.Host;

public sealed class TextreeHostOptions
{
    // "fast" = multilingual-e5-small (384d, ~470MB). Confirmed LMSupply preset alias
    // (Filer FilerRagOptions: "fast"=e5-small/384d, "default"=e5-base/768d, "quality"=BGE-M3/1024d).
    public string EmbeddingModel { get; set; } = "fast";

    /// <summary>
    /// The environment variable that moves <see cref="IndexRoot"/> — for a test run, which must not
    /// leave an index for every temporary folder in the user's own application data.
    /// </summary>
    public const string IndexRootSetting = "TEXTREE_INDEX_ROOT";

    public string IndexRoot { get; set; } =
        Environment.GetEnvironmentVariable(IndexRootSetting) is { Length: > 0 } root
            ? root
            : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                           "textree", "index");
}
