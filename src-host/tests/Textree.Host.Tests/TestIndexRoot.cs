using System.Runtime.CompilerServices;
using FluentAssertions;
using Textree.Host;
using Xunit;

/// <summary>
/// Every host these tests start keeps its indexes in a folder of the test run's own, removed when the
/// run ends — not in the application data of whoever runs the tests.
/// </summary>
internal static class TestIndexRoot
{
    public static readonly string Path =
        System.IO.Path.Combine(System.IO.Path.GetTempPath(), "textree-host-tests-index-" + Guid.NewGuid().ToString("N"));

    [ModuleInitializer]
    internal static void Use()
    {
        Environment.SetEnvironmentVariable(TextreeHostOptions.IndexRootSetting, Path);
        AppDomain.CurrentDomain.ProcessExit += (_, _) =>
        {
            try { Directory.Delete(Path, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        };
    }
}

public class TextreeHostOptionsTests
{
    [Fact]
    public void The_index_root_follows_the_environment()
    {
        new TextreeHostOptions().IndexRoot.Should().Be(TestIndexRoot.Path);
    }
}
