using System.Collections.Concurrent;

namespace Textree.Ingest;

/// <summary>
/// Where folder summaries are kept between ingests. A summary is stored with the version of the
/// part of the hierarchy it describes and is not used once that changes. The consumer decides
/// where it lives (memory, a cache folder of its own) — never inside the knowledge folder.
/// </summary>
public interface ISummaryCache
{
    string? Get(string folder, string version);
    void Set(string folder, string version, string summary);
}

/// <summary>Summaries in memory, for the life of the process.</summary>
public sealed class MemorySummaryCache : ISummaryCache
{
    private readonly ConcurrentDictionary<string, (string Version, string Summary)> _entries = new();

    public string? Get(string folder, string version) =>
        _entries.TryGetValue(folder, out var e) && e.Version == version ? e.Summary : null;

    public void Set(string folder, string version, string summary) => _entries[folder] = (version, summary);
}

/// <summary>A proposal the harness made: what it offered, for a consumer that records how people settle it.</summary>
/// <param name="Id">Identifies the proposal; the consumer settles it under this id.</param>
/// <param name="At">When it was made.</param>
/// <param name="Placement">The places offered.</param>
/// <param name="Source">The request's source, as given.</param>
public sealed record Proposal(string Id, DateTimeOffset At, IReadOnlyList<Placement> Placement, string? Source);

/// <summary>
/// Receives each proposal. A consumer that keeps settled records (accepted, corrected to another
/// place, rejected) writes them under the proposal's id; the harness keeps nothing.
/// </summary>
public interface IProposalLog
{
    void Proposed(Proposal proposal);
}
