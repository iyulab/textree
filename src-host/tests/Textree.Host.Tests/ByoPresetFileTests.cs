using System.Text.Json;
using FluentAssertions;
using IronProw.IronHive;
using Xunit;

namespace Textree.Host.Tests;

/// <summary>
/// Settings offers BYO presets from <c>src/lib/byo-presets.json</c>; this host serves them from
/// IronProw's catalogue. The two must say the same thing, or Settings prefills an address, or asks
/// for a key, differently from what the helper then does.
/// </summary>
public class ByoPresetFileTests
{
    private sealed record Entry(string Label, string DefaultBaseUrl, bool ApiKeyRequired);

    private static Dictionary<string, Entry> PresetFile()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null && !File.Exists(Path.Combine(dir.FullName, "src", "lib", "byo-presets.json")))
            dir = dir.Parent;
        dir.Should().NotBeNull("the test runs inside the repository");
        var json = File.ReadAllText(Path.Combine(dir!.FullName, "src", "lib", "byo-presets.json"));
        return JsonSerializer.Deserialize<Dictionary<string, Entry>>(json, new JsonSerializerOptions(JsonSerializerDefaults.Web))!;
    }

    [Fact]
    public void Settings_offers_exactly_the_catalogues_presets()
    {
        PresetFile().Keys.Should().BeEquivalentTo(ByoPresets.All.Select(p => p.Id));
    }

    [Fact]
    public void Each_preset_starts_with_the_catalogues_address_and_asks_for_a_key_when_it_does()
    {
        foreach (var (id, entry) in PresetFile())
        {
            var preset = ByoPresets.Find(id)!;
            entry.DefaultBaseUrl.Should().Be(preset.DefaultBaseUrl ?? "", $"the address {id} starts with");
            (entry.DefaultBaseUrl == "").Should().Be(preset.BaseUrlRequired, $"whether {id} needs an address");
            entry.ApiKeyRequired.Should().Be(preset.ApiKeyRequired, $"whether {id} needs a key");
        }
    }
}
