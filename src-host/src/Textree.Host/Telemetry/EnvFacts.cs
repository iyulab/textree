using System.Reflection;
using System.Runtime.InteropServices;

namespace Textree.Host.Telemetry;

/// <summary>
/// Structured, content-free environment facts. Every field is derived from runtime APIs,
/// never from user input — so no path, name, or identifier can enter by construction.
/// HardwareClass is a coarse bucket (the ~5GB model / DirectML story needs RAM class, not exact specs).
/// </summary>
public sealed record EnvFacts(string AppVersion, string Os, string Arch, string HardwareClass)
{
    public static EnvFacts Current()
    {
        var version = typeof(EnvFacts).Assembly
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion
            ?? typeof(EnvFacts).Assembly.GetName().Version?.ToString()
            ?? "0.0.0";
        // Drop any build metadata (+sha) so the value stays a plain version, never a path.
        var plusIndex = version.IndexOf('+');
        if (plusIndex >= 0) version = version[..plusIndex];

        return new EnvFacts(
            AppVersion: version,
            Os: OsName(),
            Arch: RuntimeInformation.OSArchitecture.ToString(),
            HardwareClass: ComputeHardwareClass());
    }

    private static string OsName()
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) return "Windows";
        if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX)) return "macOS";
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux)) return "Linux";
        return "Unknown";
    }

    private static string ComputeHardwareClass()
    {
        var cpu = Environment.ProcessorCount;
        var ramGb = GC.GetGCMemoryInfo().TotalAvailableMemoryBytes / (1024L * 1024 * 1024);
        var ramBucket = ramGb switch
        {
            <= 8 => 8,
            <= 16 => 16,
            <= 32 => 32,
            _ => 64,
        };
        return $"cpu{cpu}_ram{ramBucket}";
    }
}
