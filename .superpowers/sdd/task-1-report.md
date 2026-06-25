# Task 1 Report: ModelStatus shared download/readiness state + IProgress adapter

## Status
DONE_WITH_CONCERNS

## Files Created
- `src-host/src/Textree.Host/Rag/ModelStatus.cs`
- `src-host/tests/Textree.Host.Tests/ModelStatusTests.cs`

## TDD Evidence

### RED (Step 2)
Command: `dotnet test src-host/TextreeHost.slnx --filter ModelStatusTests`

Result: Build failure — `ModelStatus` and `ModelPhase` not found (expected compile errors). Also surfaced a critical discrepancy (see Concerns below).

```
error CS0246: The type or namespace name 'ModelStatus' could not be found
error CS0103: The name 'ModelPhase' does not exist in the current context
error CS0200: Property or indexer 'DownloadProgress.OverallPercentComplete' cannot be assigned to -- it is read only
```

The third error (`CS0200`) revealed that `DownloadProgress.OverallPercentComplete` is a **computed, read-only property**, not settable via object initializer as the brief's test assumed.

### GREEN (Step 4)
Command: `dotnet test src-host/TextreeHost.slnx --filter ModelStatusTests`

Result: `Passed! - Failed: 0, Passed: 3, Skipped: 0, Total: 3, Duration: 204 ms`

### Full suite (regression check)
Command: `dotnet test src-host/TextreeHost.slnx`

Result: `Passed! - Failed: 0, Passed: 15, Skipped: 1, Total: 16, Duration: 23 s`
Build output: `0 Warning(s)  0 Error(s)`

All 3 new ModelStatusTests are included in the 15 passed. Zero regressions.

## What Was Implemented

**`ModelStatus.cs`** (namespace `Textree.Host.Rag`):
- `enum ModelPhase { Idle, Downloading, Loading, Ready, Error }`
- `record ModelSnapshot(...)` with static `Idle` default
- `ModelStatus` sealed class with:
  - `Embedder` / `Generator` snapshot properties via `Volatile.Read`
  - `IProgress<DownloadProgress>` adapters using `SyncProgress` nested class (synchronous, not `System.Progress<T>`)
  - `SetEmbedderPhase`, `SetGeneratorPhase`, `SetEmbedderError`, `SetGeneratorError` mutators
  - Atomic snapshot swaps via `Volatile.Write`

**`ModelStatusTests.cs`** — 3 xUnit facts, same using/namespace style as `VaultHashTests.cs`.

## Test Correction (Critical Deviation from Brief)

The brief's test code contained:
```csharp
status.EmbedderProgress.Report(new DownloadProgress
{
    OverallPercentComplete = 41.4,  // CS0200 — read-only, cannot assign
    ...
});
Assert.Equal(41.4, snap.OverallPercent, 1);
```

`LMSupply.DownloadProgress.OverallPercentComplete` is a computed, read-only property (derived from `BytesDownloaded/TotalBytes` x file-count weighting). It is NOT settable. `PercentComplete` is also read-only.

Fix applied: removed the unassignable `OverallPercentComplete = 41.4` from the object initializer, and changed the assert to compare against the source object's actual computed value:
```csharp
Assert.Equal(progress.OverallPercentComplete, snap.OverallPercent);
```

This is a stronger test (catches anyone substituting `PercentComplete` for `OverallPercentComplete` in `Report`) and is deterministic.

**The production `Report` method correctly reads `p.OverallPercentComplete`** — this is the right property to use for overall multi-file progress.

## Concerns for Downstream Tasks

**`LMSupply.DownloadProgress` property settability (v0.34.20):**

From reflection on `LMSupply.Core` 0.34.20:
- **Settable (init-only)**: `FileName`, `BytesDownloaded`, `TotalBytes`, `CurrentFileIndex`, `TotalFileCount`, `BytesPerSecond`, `EstimatedRemaining`, `Phase`
- **Computed (read-only)**: `PercentComplete`, `OverallPercentComplete`, `SpeedDisplay`, `EtaDisplay`

The brief stated properties `OverallPercentComplete`, `BytesDownloaded`, `TotalBytes`, `CurrentFileIndex`, `TotalFileCount`, `FileName` "are accurate" — they exist, but `OverallPercentComplete` is NOT settable. Any downstream task that constructs `DownloadProgress` test fixtures must use only the settable properties.

With `BytesDownloaded=1_200_000_000`, `TotalBytes=2_900_000_000`, `CurrentFileIndex=2`, `TotalFileCount=3`, the actual values are `PercentComplete≈41.38` and `OverallPercentComplete≈47.13` (not `41.4` as the brief assumed).

## Commit
`afc738b feat(host): ModelStatus shared download/readiness state + IProgress adapter`

---
**Date:** 2026-06-25
**Branch:** feat/cold-model-download-progress
**Directory:** D:\data\textree-umbrella\textree\

## Task 1 Fix Wave

### Changes Applied

1. **`SetPhase` Error guard** (`ModelStatus.cs`): `SetPhase` now throws `ArgumentException` if called with `ModelPhase.Error`, forcing callers to use `SetEmbedderError`/`SetGeneratorError`. Non-error phases now unconditionally clear `Error = null` (the prior conditional was the inconsistency footgun). Keeps `SetError` as the sole correct path into `Error` state.

2. **Concurrency comment** (`ModelStatus.cs`): Added `// Single writer per model slot (the background load loop); /health only reads. No CAS needed.` above `_embedder`/`_generator` field declarations.

3. **Report method comment** (`ModelStatus.cs`): Added `// Progress callbacks mean bytes are moving → Downloading; the load loop drives Loading/Ready/Error explicitly.` above the `Report` method.

4. **New test** (`ModelStatusTests.cs`): Added `SetEmbedderPhase_Error_throws_ArgumentException` — asserts that `SetEmbedderPhase(ModelPhase.Error)` throws `ArgumentException`, with a comment pointing to the existing `SetEmbedderError` test as coverage for the correct error path.

### Test Commands and Output

**Targeted:**
```
dotnet test src-host/TextreeHost.slnx --filter ModelStatusTests
Passed! - Failed: 0, Passed: 4, Skipped: 0, Total: 4, Duration: 8 ms
```

**Full suite (regression + warnings check):**
```
dotnet test src-host/TextreeHost.slnx
Passed! - Failed: 0, Passed: 16, Skipped: 1, Total: 17, Duration: 25 s
0 warnings, 0 errors
```

(Skipped 1 = `TextGeneratorBenchTests.Measure_tokens_per_second_on_representative_prompt`, pre-existing skip — not touched.)

### Commit
`fix(host): guard ModelStatus.SetPhase against Error phase + concurrency comments`
