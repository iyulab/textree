# Task 1 Report — Unblock Host Build + Dogfood Embedding Adapter

**Status:** DONE  
**Commits:** `238bb67` (build unblock) · `4a1fc6b` (test CPU EP fix)  
**Date:** 2026-06-20

---

## What Was Done

### Step 1: Reproduced the build failure
`dotnet build src-host/TextreeHost.slnx -c Release` failed with:
```
error NU1903: Warning As Error: Package 'SQLitePCLRaw.lib.e_sqlite3' 2.1.11
has a known high severity vulnerability, https://github.com/advisories/GHSA-2m69-gcr7-jv3q
```
Both `Textree.Host.csproj` AND `Textree.Host.Tests.csproj` error independently (each resolves the transitive).

### Step 2: Resolved the advisory — Fallback (NoWarn) used

**Investigation:**
- `dotnet package search SQLitePCLRaw.lib.e_sqlite3 --prerelease` → latest stable AND prerelease is 2.1.11 (the vulnerable version). No patched version exists.
- Transitive pull-in path confirmed via `project.assets.json`: `Microsoft.Data.Sqlite 10.0.8` → `SQLitePCLRaw.bundle_e_sqlite3 2.1.11` → `SQLitePCLRaw.lib.e_sqlite3 2.1.11`.
- Bumping `Microsoft.Data.Sqlite` to `10.0.9` would not help (still pulls 2.1.11). `sqlite-vec` (0.1.7-alpha.2.1) has no SQLitePCLRaw dependency — not the source.

**Resolution chosen:** Fallback (documented in brief) — `NoWarn=NU1903` added to `Directory.Build.props` (solution-wide), NOT to individual `.csproj` files. Rationale: NU1903 is project-local; adding to only one `.csproj` would leave the test project failing at restore. Solution-wide via `Directory.Build.props` is the correct scope.

```xml
<!-- TODO: remove once SQLitePCLRaw ships a patched SQLitePCLRaw.lib.e_sqlite3 (>2.1.11).
     No patched version exists as of 2026-06-20; tracked: https://github.com/advisories/GHSA-2m69-gcr7-jv3q
     The transitive pull-in path is Microsoft.Data.Sqlite -> SQLitePCLRaw.bundle_e_sqlite3 -> SQLitePCLRaw.lib.e_sqlite3 2.1.11.
     Applied solution-wide (Directory.Build.props) because both Textree.Host and Textree.Host.Tests
     transitively resolve the vulnerable package and NU1903 is project-local — NoWarn in a single csproj
     would still fail restore for the other project. -->
<NoWarn>$(NoWarn);NU1903</NoWarn>
```

**File modified:** `src-host/Directory.Build.props`

### Step 3: Build passes

```
dotnet build src-host/TextreeHost.slnx -c Release
Build succeeded. 0 Warning(s). 0 Error(s).
```

### Step 4: dotnet test — ALL 5 PASS

**VaultHash tests (3):** Pass (no model required).

**VaultManagerScopeTests (2 integration tests):** Initially BLOCKED by ONNX DirectML GPU crash. Resolved by passing `ExecutionProvider.Cpu` in the test's `LocalEmbedder.LoadAsync` call (see Step 4b below).

**Final result:**
```
Test Run Successful.
Total tests: 5
     Passed: 5
Total time: 14.1309 Seconds
```

### Step 4b: Resolve ONNX DML crash in integration tests

**Root cause:** `LocalEmbedder.LoadAsync(opts.EmbeddingModel)` (no options) defaults to `ExecutionProvider.Auto`, which tries DirectML on Windows. This machine's GPU driver produces:
```
[E:onnxruntime:] Non-zero status code returned while running LayerNormalization node.
Status Message: MLOperatorAuthorImpl.cpp(2508): The parameter is incorrect. (0x80070057)
```
The crash kills the testhost. It is a machine-level environment issue, not a code regression.

**Fix:** Inspected `LMSupply.Embedder.EmbedderOptions` via reflection — it inherits `LMSupplyOptionsBase` which exposes `Provider: LMSupply.ExecutionProvider`. Enum has `Cpu`, `DirectML`, `Cuda`, `CoreML`, `Auto` values.

Modified `VaultManagerScopeTests.cs` to pass `new EmbedderOptions { Provider = ExecutionProvider.Cpu }`:
```csharp
var model = await LocalEmbedder.LoadAsync(
    opts.EmbeddingModel,
    new EmbedderOptions { Provider = LMSupply.ExecutionProvider.Cpu });
```

**This is test-only:** Production code (`Program.cs`, `EmbeddingService.cs`) uses `ExecutionProvider.Auto` (GPU-when-available). The test override ensures integration tests run in any environment, including machines without a compatible GPU/DirectML driver. No production behavior changed.

**Commit:** `4a1fc6b` — `test(host): force CPU execution provider in integration tests`

### Step 5-6: Dogfood adapter swap — SKIPPED (pre-compile incompatibility found)

**Investigation of `FluxIndex.Providers.LMSupply 0.13.19`:**
Inspected `FluxIndex.Providers.LMSupply.xml` (shipped with the nupkg) to enumerate the public API of `LMSupplyEmbeddingService`:
- Constructor: `LMSupplyEmbeddingService(IEmbeddingModel model)` ✓ (same as hand-rolled)
- `GetEmbeddingDimension()` ✓ (from IEmbeddingService interface)
- `GetModelName()` ✓
- `GetProviderName()` ✓
- `GenerateEmbeddingsBatchAsync(...)` ✓
- `DisposeAsync()` ✓ (IAsyncDisposable)
- **`Dimensions` property: ABSENT** ✗

The hand-rolled `LmSupplyEmbeddingService` exposes `public int Dimensions => _model.Dimensions;` which `VaultManager.cs` uses in two places:
- Line 34: `public bool EmbedderReady => _embedder.Dimensions > 0;`
- Line 89: `o.VectorDimension = _embedder.Dimensions;`

Swapping to the published adapter without modifying `VaultManager.cs` would be a compile error. The brief explicitly says "REVERT the swap... do not refactor VaultManager to force the fit (scope creep)."

**Action taken:** Swap skipped entirely (did not add the package ref). Added NOTE comment in `EmbeddingService.cs`:
```csharp
// NOTE: FluxIndex.Providers.LMSupply.LMSupplyEmbeddingService exists but signature diverged at 0.13.19;
// revisit. The published adapter (FluxIndex.Providers.LMSupply.Services.LMSupplyEmbeddingService) does
// not expose a public Dimensions property — only GetEmbeddingDimension() via the interface.
// VaultManager depends on .Dimensions directly (EmbedderReady and SQLite vector dimension), so
// dropping this hand-rolled adapter would require refactoring VaultManager as well. Keeping hand-rolled
// until the published adapter aligns or VaultManager is updated to use GetEmbeddingDimension().
```

**File modified:** `src-host/src/Textree.Host/Rag/EmbeddingService.cs`

### Step 7: Commits

```
238bb67 fix(host): unblock build — suppress NU1903 SQLitePCLRaw advisory solution-wide
4a1fc6b test(host): force CPU execution provider in integration tests
```

Commit message for `238bb67` intentionally omits "adopt published LMSupply embedding adapter" (adapter swap was not done).

---

## Files Changed

| File | Change |
|---|---|
| `src-host/Directory.Build.props` | Added `NoWarn=NU1903` with TODO comment explaining the advisory and removal condition |
| `src-host/src/Textree.Host/Rag/EmbeddingService.cs` | Added NOTE comment about published adapter signature divergence |
| `src-host/tests/Textree.Host.Tests/VaultManagerScopeTests.cs` | Force `ExecutionProvider.Cpu` in `LocalEmbedder.LoadAsync` to avoid DML crash on this machine |

No endpoint, DTO, or production runtime behavior changed. No new dependencies added.

---

## Concerns

### CONCERN 1 (RESOLVED): Integration tests blocked by ONNX DirectML GPU crash

Resolved by passing `ExecutionProvider.Cpu` in the test's `LocalEmbedder.LoadAsync` call (see Step 4b). All 5 tests now pass. Production code is unaffected.

### CONCERN 2: Dogfood adapter swap deferred

`FluxIndex.Providers.LMSupply 0.13.19` `LMSupplyEmbeddingService` lacks a public `.Dimensions` property. The swap can only proceed when either:
- The published adapter adds `public int Dimensions { get; }`, OR
- `VaultManager.cs` is refactored to use `GetEmbeddingDimension()` instead of `.Dimensions` directly

This is a small refactor that should be in scope for a future task.

---

## Test Summary

| Test | Result |
|---|---|
| VaultHashTests.SamePathNormalizedToSameHash | PASSED |
| VaultHashTests.DifferentPathsDifferHash | PASSED |
| VaultHashTests.HashIs16HexChars | PASSED |
| VaultManagerScopeTests.SubScopeExcludesRootNotes | PASSED |
| VaultManagerScopeTests.ReindexIsIdempotent_NoDuplicateAccumulation | PASSED |

---

## Commands Run (Exact)

```powershell
# Step 1: Confirm failure
dotnet build src-host/TextreeHost.slnx -c Release
# → FAIL: NU1903 on both projects

# Step 2: Investigation
dotnet package search "SQLitePCLRaw.lib.e_sqlite3" --prerelease --take 5
# → Latest: 2.1.11 (vulnerable, no newer version)

dotnet package search "Microsoft.Data.Sqlite" --take 5
# → Latest: 10.0.9 (still pulls SQLitePCLRaw 2.1.11)

# Inspected project.assets.json to confirm transitive path

# Step 3: Verify build after fix
dotnet build src-host/TextreeHost.slnx -c Release
# → PASS: 0 errors, 0 warnings

# Step 4: Run tests (initial 3 attempts — all DML crash)
dotnet test src-host/TextreeHost.slnx --verbosity normal
# → Run 1: 3 passed, 2 blocked (DML crash after 20+ min hang)
# → Run 2: 3 passed, testhost crashed (52 seconds, DML error)
# → Run 3: 3 passed, testhost crashed (same DML error, ORT_DISABLE_DML had no effect)

# Step 4b: Force CPU in test; re-run
# Modified VaultManagerScopeTests.cs: added EmbedderOptions { Provider = ExecutionProvider.Cpu }
dotnet test src-host/TextreeHost.slnx --verbosity normal
# → 5 passed (14.1 seconds) — Test Run Successful
```
