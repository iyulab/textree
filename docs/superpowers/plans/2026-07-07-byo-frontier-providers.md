# BYO Frontier Providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the BYO ("custom AI server") flow target frontier cloud models (OpenAI, Anthropic, Gemini, Grok) by adopting iron-prow's `IronProw.IronHive` bridge, instead of textree's host constructing IronHive generators itself.

**Architecture:** The host stops hand-rolling provider construction. `Program.cs` registers exactly one provider on an `IronProwBuilder` via the matching `AddIronHive*` bridge call, and the resulting `IChatClient` (a single-provider, therefore inert, `SelectingChatClient`) is adapted back into the host's existing `ITextGenerator` interface. iron-prow itself is unchanged. The frontend gains four presets.

**Tech Stack:** Svelte 5 (runes) + TypeScript (Vitest), Rust/Tauri 2 (`cargo test`), .NET 10 host (`dotnet test`, `TreatWarningsAsErrors=true`), Playwright (WebView2 CDP).

## Global Constraints

- Public repo (`iyulab/textree`): all code/comments/UI copy/commit messages in **ENGLISH**.
- Frontend styling: `var(--token)` from `src/lib/styles/tokens.css` only — zero hardcoded colors/spacing.
- Frontend layering: components → runes stores → pure helpers; tests import only pure helpers.
- Host: `TreatWarningsAsErrors=true`. iron-prow is adopted as-is; **no change to any iron-prow repo/package**.
- Identity: everything stays inside **Settings ▸ Advanced: custom AI server**; no frontier provider names on the product surface (README/homepage/welcome).
- Gates before merge: `npm run check` (0/0), `npm run test:unit`, `cargo test`, `dotnet test`, E2E, build.
- Commits: Conventional Commits. Do NOT push (manual).

## iron-prow reference facts (verified from the clone + nuget xml docs)

- `services.AddIronProw()` returns an `IronProwBuilder` and registers `IChatClient` → `SelectingChatClient` (which wraps each provider attempt in `GuardedChatClient(NullGuard)` + `ResilienceChatClient`). Default guard is `NullGuard` (no-op).
- Bridge extension methods (namespace `IronProw.IronHive`), signature `(this IronProwBuilder, string id, int priority, string model, Action<TConfig> configure)`:
  - `AddIronHiveOpenAI` → `OpenAIConfig` (Frontier)
  - `AddIronHiveAnthropic` → `AnthropicConfig` (Frontier); `AnthropicConfig.BaseUrl` optional (SDK defaults to api.anthropic.com), `ApiKey` required
  - `AddIronHiveGoogleAI` → `GoogleAIConfig` (Frontier)
  - `AddIronHiveGpuStack` → `GpuStackConfig` (Lan, key-optional)
  - `AddIronHiveOpenAICompatible` → `OpenAICompatibleConfig` (Lan, key-optional)
- `IronProw.IronHive` transitively references `IronHive.Providers.{Anthropic,GoogleAI,OpenAI,OpenAI.Compatible}` + `IronHive.Core`.
- **Behavior change to note:** the single-provider frontier path is wrapped in `ResilienceChatClient`, so cloud calls now get iron-prow's retry policy (they did not before). This is acceptable/desirable but must be called out in the host task.

---

### Task 1: Extend the ByoPreset union (frontend)

**Files:**
- Modify: `src/lib/byoConfig.ts`
- Test: `src/lib/byoConfig.test.ts`

**Interfaces:**
- Produces: `ByoPreset = "ollama" | "gpustack" | "openai" | "anthropic" | "gemini" | "grok" | "custom"` and an `isByoPreset` guard accepting all seven.

- [ ] **Step 1: Write the failing test**

Add to `src/lib/byoConfig.test.ts` (read the file first for its existing `describe`/import style):

```ts
import { getByoConfig, setByoConfig } from "./byoConfig";

describe("ByoPreset frontier presets", () => {
  for (const preset of ["openai", "anthropic", "gemini", "grok"] as const) {
    it(`round-trips the ${preset} preset`, () => {
      setByoConfig({ preset, baseUrl: "https://example.test/v1", model: "m" });
      expect(getByoConfig()).toEqual({
        preset, baseUrl: "https://example.test/v1", model: "m",
      });
    });
  }
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/lib/byoConfig.test.ts`
Expected: FAIL — TS narrowing / `isByoPreset` rejects the new preset strings (or `getByoConfig` returns null because `isByoPreset` is false).

- [ ] **Step 3: Implement**

In `src/lib/byoConfig.ts`, change the type and guard:

```ts
export type ByoPreset =
  | "ollama" | "gpustack" | "openai" | "anthropic" | "gemini" | "grok" | "custom";
```

```ts
function isByoPreset(value: string | null): value is ByoPreset {
  return (
    value === "ollama" || value === "gpustack" || value === "openai" ||
    value === "anthropic" || value === "gemini" || value === "grok" ||
    value === "custom"
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/lib/byoConfig.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib/byoConfig.ts src/lib/byoConfig.test.ts
git commit -m "feat: add openai/anthropic/gemini/grok to ByoPreset"
```

---

### Task 2: Preset defaults, badges, and blank-baseUrl validity (frontend helpers)

**Files:**
- Modify: `src/lib/settings.helpers.ts`
- Test: `src/lib/settings.helpers.test.ts`

**Interfaces:**
- Consumes: `ByoPreset` (Task 1).
- Produces: `presetDefaults(preset)` returning baseUrl prefills for the new presets; `byoProviderBadge(activeProvider)` labels; `isValidByoUrl` accepting blank **only for anthropic** via a new `isValidByoUrlForPreset(preset, url)` (keep `isValidByoUrl` for the generic case).

- [ ] **Step 1: Write the failing tests**

Add to `src/lib/settings.helpers.test.ts` (match its existing import/`describe` idioms):

```ts
describe("presetDefaults frontier baseUrls", () => {
  it("prefills openai", () => {
    expect(presetDefaults("openai").baseUrl).toBe("https://api.openai.com/v1");
  });
  it("prefills grok", () => {
    expect(presetDefaults("grok").baseUrl).toBe("https://api.x.ai/v1");
  });
  it("prefills gemini", () => {
    expect(presetDefaults("gemini").baseUrl).toBe(
      "https://generativelanguage.googleapis.com/v1beta/openai");
  });
  it("leaves anthropic baseUrl blank (SDK default)", () => {
    expect(presetDefaults("anthropic").baseUrl).toBe("");
  });
});

describe("isValidByoUrlForPreset", () => {
  it("accepts a blank baseUrl for anthropic", () => {
    expect(isValidByoUrlForPreset("anthropic", "")).toBe(true);
  });
  it("rejects a blank baseUrl for non-anthropic presets", () => {
    expect(isValidByoUrlForPreset("openai", "")).toBe(false);
  });
  it("accepts a valid http(s) baseUrl for any preset", () => {
    expect(isValidByoUrlForPreset("openai", "https://api.openai.com/v1")).toBe(true);
  });
});
```

Also import `isValidByoUrlForPreset` and `presetDefaults` at the top if not already imported.

- [ ] **Step 2: Run to verify it fails**

Run: `npx vitest run src/lib/settings.helpers.test.ts`
Expected: FAIL — `isValidByoUrlForPreset` undefined and `presetDefaults` missing the new keys.

- [ ] **Step 3: Implement**

In `src/lib/settings.helpers.ts`, extend `PRESET_DEFAULT_BASE_URL`:

```ts
const PRESET_DEFAULT_BASE_URL: Record<ByoPreset, string> = {
  ollama: "http://localhost:11434",
  gpustack: "http://localhost:8080",
  openai: "https://api.openai.com/v1",
  anthropic: "",
  gemini: "https://generativelanguage.googleapis.com/v1beta/openai",
  grok: "https://api.x.ai/v1",
  custom: "",
};
```

Add the preset-aware validity helper next to `isValidByoUrl`:

```ts
/** Anthropic's SDK supplies its own base URL, so a blank value is valid for that preset only.
 * Every other preset needs a concrete http(s) endpoint. */
export function isValidByoUrlForPreset(preset: ByoPreset, url: string): boolean {
  if (preset === "anthropic" && url.trim() === "") return true;
  return isValidByoUrl(url);
}
```

Extend `byoProviderBadge` to label the new providers (keep existing branches):

```ts
export function byoProviderBadge(activeProvider: string): string {
  if (activeProvider === "" || activeProvider === "local") return "Local model";
  const labels: Record<string, string> = {
    ollama: "Ollama", gpustack: "GPUStack", openai: "OpenAI",
    anthropic: "Anthropic", gemini: "Gemini", grok: "Grok",
  };
  return `Custom server (${labels[activeProvider] ?? activeProvider})`;
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `npx vitest run src/lib/settings.helpers.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib/settings.helpers.ts src/lib/settings.helpers.test.ts
git commit -m "feat: preset defaults/badges for frontier providers, blank-URL validity for anthropic"
```

---

### Task 3: Settings UI — 7 presets, wrap, anthropic optional baseUrl (frontend)

**Files:**
- Modify: `src/lib/Settings.svelte`

**Interfaces:**
- Consumes: `presetDefaults`, `isValidByoUrlForPreset`, `byoProviderBadge` (Task 2); the extended `ByoPreset` (Task 1).

- [ ] **Step 1: Add the new presets to the segmented control**

In `src/lib/Settings.svelte`, the preset `seg-group` currently iterates `["ollama", "gpustack", "custom"]`. Change it to:

```svelte
{#each (["ollama", "gpustack", "openai", "anthropic", "gemini", "grok", "custom"] as const) as p (p)}
```

- [ ] **Step 2: Make the base-URL requirement preset-aware**

The `onSaveByo` disabled-gate and the Base URL label currently use `isValidByoUrl(byoBaseUrl)`. Import `isValidByoUrlForPreset` (add to the existing `settings.helpers` import) and replace the Save button's `disabled` URL check and the Test button's `disabled` check to use it:

```svelte
disabled={byoSaving || !isValidByoUrlForPreset(byoPreset, byoBaseUrl) || !isValidByoModel(byoModel)}
```
(Save button) and for the Test button:
```svelte
disabled={!isValidByoUrlForPreset(byoPreset, byoBaseUrl)}
```

Update the Base URL label to signal optionality for anthropic:

```svelte
<label>
  Base URL{#if byoPreset === "anthropic"} (optional){/if}
  <input type="text" bind:value={byoBaseUrl}
    placeholder={byoPreset === "anthropic" ? "leave blank for the default endpoint" : "http://localhost:11434"} />
</label>
```

- [ ] **Step 3: Allow the segmented control to wrap**

In the `<style>` block, find `.seg-group` and add `flex-wrap: wrap;` (keep its existing rules). If it has no explicit `display: flex`, add `display: flex; flex-wrap: wrap; gap: var(--sp-1);` — grep the current `.seg-group` rule first and merge, don't duplicate.

- [ ] **Step 4: Verify type-check**

Run: `npm run check`
Expected: `0 errors, 0 warnings`.

- [ ] **Step 5: Commit**

```bash
git add src/lib/Settings.svelte
git commit -m "feat: expose frontier presets in Settings, wrap preset control, optional anthropic base URL"
```

---

### Task 4: Anthropic-aware connection test (Rust)

**Files:**
- Modify: `src-tauri/src/host.rs` (`test_byo_connection`, around line 535)
- Test: `src-tauri/src/host.rs` (a `#[cfg(test)]` unit test, following the file's existing test module style — grep `mod tests` in the file)

**Interfaces:**
- Consumes: nothing new.
- Produces: `test_byo_connection` returns `Ok(())` immediately for the `anthropic` preset (its base URL is blank and its auth is `x-api-key`, not the Bearer `/v1/models` probe this function does — real validation happens on the first `/chat` call, matching the host's lazy-failure model).

- [ ] **Step 1: Write the failing test**

Add a unit test near the other `host.rs` tests asserting anthropic short-circuits without a network call:

```rust
#[test]
fn test_byo_connection_anthropic_is_ok_without_base_url() {
    // Anthropic uses x-api-key + a fixed endpoint, not the Bearer /v1/models probe.
    // A blank base URL must not be treated as an unreachable server.
    let r = test_byo_connection("anthropic".to_string(), "".to_string(), Some("sk-ant".to_string()));
    assert!(r.is_ok());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml test_byo_connection_anthropic`
Expected: FAIL — the current code builds `"/v1/models"` onto a blank base URL and the `ureq::get` errors (returns `Err`).

- [ ] **Step 3: Implement**

At the top of `test_byo_connection`, short-circuit anthropic:

```rust
pub fn test_byo_connection(preset: String, base_url: String, api_key: Option<String>) -> Result<(), String> {
    // Anthropic's endpoint is fixed and uses x-api-key (not the Bearer /v1/models probe below);
    // treat a configured anthropic preset as reachable and let the first real request validate.
    if preset.eq_ignore_ascii_case("anthropic") {
        return Ok(());
    }
    let path = if preset.eq_ignore_ascii_case("gpustack") { "/v1-openai/models" } else { "/v1/models" };
    // ... unchanged ...
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml test_byo_connection_anthropic`
Expected: PASS. Then `cargo test --manifest-path src-tauri/Cargo.toml` — full suite green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/host.rs
git commit -m "feat: short-circuit anthropic in the BYO connection test (fixed endpoint, x-api-key)"
```

---

### Task 5: Host — reference IronProw.IronHive and add the preset→bridge mapping (.NET)

**Files:**
- Modify: `src-host/src/Textree.Host/Textree.Host.csproj`
- Create: `src-host/src/Textree.Host/Rag/ByoProviderRegistration.cs`
- Test: `src-host/tests/Textree.Host.Tests/ByoProviderRegistrationTests.cs`

**Interfaces:**
- Produces: `ByoProviderRegistration.Register(IronProwBuilder builder, string preset, string baseUrl, string? apiKey, string model)` — calls the correct `AddIronHive*` for the preset, sets baseUrl/apiKey/model, and returns the builder. Consumed by Task 6's `Program.cs`.

- [ ] **Step 1: Swap the package reference**

In `src-host/src/Textree.Host/Textree.Host.csproj`, replace:
```xml
    <PackageReference Include="IronHive.Core" />
    <PackageReference Include="IronHive.Providers.OpenAI.Compatible" />
```
with:
```xml
    <PackageReference Include="IronProw.IronHive" />
```
(`IronProw.IronHive` brings `IronHive.Core` + all provider packages transitively. Add the version via the repo's central `Directory.Packages.props` if that's how versions are pinned — grep `IronHive.Core` there and mirror the style.)

- [ ] **Step 2: Write the failing test**

Create `src-host/tests/Textree.Host.Tests/ByoProviderRegistrationTests.cs`. It asserts each preset registers a candidate of the right kind (mirroring iron-prow's own `IronHiveProviderExtensionsTests`):

```csharp
using FluentAssertions;
using Microsoft.Extensions.DependencyInjection;
using IronProw.Core;
using Textree.Host.Rag;
using Xunit;

namespace Textree.Host.Tests;

public class ByoProviderRegistrationTests
{
    private static IProviderRegistry RegistryFor(string preset, string baseUrl, string model)
    {
        var services = new ServiceCollection();
        var builder = services.AddIronProw();
        ByoProviderRegistration.Register(builder, preset, baseUrl, "k", model);
        return services.BuildServiceProvider().GetRequiredService<IProviderRegistry>();
    }

    [Theory]
    [InlineData("openai", ProviderKind.Frontier)]
    [InlineData("anthropic", ProviderKind.Frontier)]
    [InlineData("gemini", ProviderKind.Frontier)]
    [InlineData("grok", ProviderKind.Lan)]        // grok → OpenAI-compatible (Lan)
    [InlineData("gpustack", ProviderKind.Lan)]
    [InlineData("ollama", ProviderKind.Lan)]
    [InlineData("custom", ProviderKind.Lan)]
    public void Register_wires_one_candidate_of_expected_kind(string preset, ProviderKind kind)
    {
        RegistryFor(preset, "https://example.test/v1", "m")
            .GetOrdered().Should().ContainSingle(r => r.Kind == kind);
    }

    [Fact]
    public void Register_anthropic_with_blank_base_url_does_not_throw()
    {
        var act = () => RegistryFor("anthropic", "", "claude-opus-4-5");
        act.Should().NotThrow();
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `dotnet test src-host/tests/Textree.Host.Tests --filter ByoProviderRegistration`
Expected: FAIL — `ByoProviderRegistration` does not exist.

- [ ] **Step 4: Implement**

Create `src-host/src/Textree.Host/Rag/ByoProviderRegistration.cs`. Map each preset to its bridge call; the priority is arbitrary (single provider) — use `100`. For key-optional Lan presets, substitute a non-empty placeholder when the key is blank (the OpenAI SDK's `ApiKeyCredential` throws on empty — the same reason the old `RemoteChatTextGenerator.EffectiveApiKey` used `"not-required"`).

```csharp
using IronProw.Core;
using IronProw.IronHive;

namespace Textree.Host.Rag;

/// <summary>
/// Maps a BYO preset to the matching IronProw.IronHive bridge registration. One provider only;
/// the gateway's selection layer is inert for a single registration.
/// </summary>
public static class ByoProviderRegistration
{
    private const int Priority = 100;

    public static IronProwBuilder Register(
        IronProwBuilder builder, string preset, string baseUrl, string? apiKey, string model)
    {
        // The OpenAI SDK wraps ApiKey in an ApiKeyCredential that throws on empty; unauthenticated
        // local servers (Ollama) don't check it, so pass a placeholder when blank.
        string key = string.IsNullOrEmpty(apiKey) ? "not-required" : apiKey;

        return preset.ToLowerInvariant() switch
        {
            "openai" => builder.AddIronHiveOpenAI("byo", Priority, model, c => { c.ApiKey = key; if (!string.IsNullOrEmpty(baseUrl)) c.BaseUrl = baseUrl; }),
            "anthropic" => builder.AddIronHiveAnthropic("byo", Priority, model, c => { c.ApiKey = key; if (!string.IsNullOrEmpty(baseUrl)) c.BaseUrl = baseUrl; }),
            "gemini" => builder.AddIronHiveGoogleAI("byo", Priority, model, c => { c.ApiKey = key; if (!string.IsNullOrEmpty(baseUrl)) c.BaseUrl = baseUrl; }),
            "gpustack" => builder.AddIronHiveGpuStack("byo", Priority, model, c => { c.ApiKey = key; c.BaseUrl = baseUrl; }),
            // grok, custom, ollama, and any unknown preset → generic OpenAI-compatible.
            _ => builder.AddIronHiveOpenAICompatible("byo", Priority, model, c => { c.ApiKey = key; c.BaseUrl = baseUrl; }),
        };
    }
}
```

> Property names (`ApiKey`/`BaseUrl`) on each `*Config` are confirmed for `AnthropicConfig` and `GpuStackConfig`; for `OpenAIConfig` / `GoogleAIConfig` / `OpenAICompatibleConfig`, confirm the exact property names against the installed package xml docs (`~/.nuget/packages/ironhive.providers.*/*/…xml`) or the iron-prow clone before finalizing — adjust the setters if a name differs. Do NOT guess.

- [ ] **Step 5: Run to verify it passes**

Run: `dotnet test src-host/tests/Textree.Host.Tests --filter ByoProviderRegistration`
Expected: PASS (8 cases). If a `*Config` property name was wrong, the compile error names it — fix and re-run.

- [ ] **Step 6: Commit**

```bash
git add src-host/src/Textree.Host/Textree.Host.csproj src-host/src/Textree.Host/Rag/ByoProviderRegistration.cs src-host/tests/Textree.Host.Tests/ByoProviderRegistrationTests.cs
git commit -m "feat: map BYO presets to IronProw.IronHive bridge registrations"
```

---

### Task 6: Host — re-wire RemoteChatTextGenerator onto the bridge IChatClient (.NET)

**Files:**
- Modify: `src-host/src/Textree.Host/Rag/RemoteChatTextGenerator.cs`
- Modify: `src-host/src/Textree.Host/Program.cs` (BYO branch, ~line 44-58)
- Test: existing `src-host/tests/Textree.Host.Tests/ChatEndpointTests.cs` must still pass (stubs `ITextGenerator`).

**Interfaces:**
- Consumes: `ByoProviderRegistration.Register` (Task 5); iron-prow `IChatClient`.
- Produces: `RemoteChatTextGenerator(IChatClient client, string model)` — an `ITextGenerator` that streams from an injected iron-prow `IChatClient`, keeping textree's `LocalSafetyChatClient` wrap, `RepetitionGuard`, and `LastError`.

- [ ] **Step 1: Re-shape RemoteChatTextGenerator to take an injected IChatClient**

Rewrite `RemoteChatTextGenerator.cs` so it no longer constructs IronHive generators. Keep the streaming loop, `RepetitionGuard`, `LastError`, and the `LocalSafetyChatClient` wrap; delete `BuildGenerator()`, `EffectiveApiKey`, the preset/baseUrl/apiKey fields, and the `ChatClientAdapter` shim (iron-prow already hands a full `IChatClient`). The constructor now takes the injected client + model:

```csharp
using System.Runtime.CompilerServices;
using System.Text;
using IronProw.LMSupply;              // LocalSafetyChatClient, LocalSafetyOptions, LazyReadinessProbe
using Microsoft.Extensions.AI;
using MeaiChatMessage = Microsoft.Extensions.AI.ChatMessage;

namespace Textree.Host.Rag;

/// <summary>
/// ITextGenerator backed by an iron-prow IChatClient (a single-provider, inert SelectingChatClient
/// built from the BYO preset). No model-download step: the endpoint is assumed running, so
/// <see cref="Ready"/> is "configured", and a bad endpoint surfaces on the first GenerateAsync.
/// </summary>
public sealed class RemoteChatTextGenerator : ITextGenerator
{
    private const int RepetitionTail = 256;
    private readonly IChatClient _client;
    private readonly string _model;
    private string? _lastError;

    public RemoteChatTextGenerator(IChatClient client, string model)
    {
        _client = client;
        _model = model;
    }

    public bool Ready => true;
    public string? LastError => Volatile.Read(ref _lastError);
    public Task PrepareAsync(CancellationToken ct) => Task.CompletedTask;

    public async IAsyncEnumerable<string> GenerateAsync(
        IReadOnlyList<ChatMessage> messages,
        GenerationOptions opts,
        [EnumeratorCancellation] CancellationToken ct)
    {
        // Keep textree's own safety wrap: iron-prow's default guard is NullGuard (no-op), so the
        // 512-token cap + readiness probe would otherwise be lost. The injected _client is a DI
        // singleton — do NOT dispose it here.
        using var chat = new LocalSafetyChatClient(
            _client,
            new LocalSafetyOptions { DefaultMaxOutputTokens = 512 },
            new LazyReadinessProbe(() => true, new[] { _model }));

        var chatMessages = messages.Select(ToChatMessage).ToList();
        var chatOptions = new ChatOptions { MaxOutputTokens = opts.MaxTokens, Temperature = opts.Temperature, ModelId = _model };

        var recent = new StringBuilder(RepetitionTail);
        IAsyncEnumerator<ChatResponseUpdate>? enumerator = null;
        try
        {
            enumerator = chat.GetStreamingResponseAsync(chatMessages, chatOptions, ct).GetAsyncEnumerator(ct);
            while (true)
            {
                bool moved;
                try { moved = await enumerator.MoveNextAsync(); }
                catch (Exception ex) when (ex is not OperationCanceledException)
                {
                    Volatile.Write(ref _lastError, ex.Message);
                    throw;
                }
                if (!moved) yield break;
                var update = enumerator.Current;
                ct.ThrowIfCancellationRequested();
                if (string.IsNullOrEmpty(update.Text)) continue;
                yield return update.Text;
                recent.Append(update.Text);
                if (recent.Length > RepetitionTail) recent.Remove(0, recent.Length - RepetitionTail);
                if (RepetitionGuard.IsDegenerate(recent.ToString())) yield break;
            }
        }
        finally
        {
            if (enumerator is not null) await enumerator.DisposeAsync();
        }
    }

    private static MeaiChatMessage ToChatMessage(Rag.ChatMessage m) => new(ParseRole(m.Role), m.Content);

    private static ChatRole ParseRole(string? role)
    {
        if (string.IsNullOrWhiteSpace(role))
            throw new ArgumentException("Message role must not be null or empty.", nameof(role));
        return role.ToLowerInvariant() switch
        {
            "system" => ChatRole.System, "assistant" => ChatRole.Assistant,
            "tool" => ChatRole.Tool, _ => ChatRole.User,
        };
    }
}
```

> Confirm `LocalSafetyChatClient` / `LocalSafetyOptions` / `LazyReadinessProbe` are in the `IronProw.LMSupply` namespace by checking the current `using` in `LocalTextGenerator.cs` (it wraps the same types). If `ModelId` on `ChatOptions` conflicts with how the model is already passed to the bridge (Task 5 passes `model` to `AddIronHive*`), keep `ModelId` here only if the bridge doesn't already bind it — verify against a first real call in Step 4's manual check; if redundant, drop `ModelId` from `chatOptions`.

- [ ] **Step 2: Wire Program.cs to build the provider via iron-prow**

In `src-host/src/Textree.Host/Program.cs`, replace the BYO branch body (the `new RemoteChatTextGenerator(byoPreset, byoBaseUrl, byoApiKey, byoModel)` singleton) with iron-prow registration + the adapter:

```csharp
if (byoPreset is not null)
{
    var byoBaseUrl = envDict.GetValueOrDefault("TEXTREE_BYO_BASE_URL") ?? "";
    var byoApiKey = envDict.GetValueOrDefault("TEXTREE_BYO_API_KEY");
    var byoModel = envDict.GetValueOrDefault("TEXTREE_BYO_MODEL") ?? "default";

    // iron-prow gateway with exactly one provider (selection inert). The bridge owns provider
    // construction (substrate); the host only adapts the resulting IChatClient into ITextGenerator.
    var prow = builder.Services.AddIronProw();
    ByoProviderRegistration.Register(prow, byoPreset, byoBaseUrl, byoApiKey, byoModel);
    builder.Services.AddSingleton<ITextGenerator>(sp =>
        new RemoteChatTextGenerator(sp.GetRequiredService<IChatClient>(), byoModel));
}
else
{
    builder.Services.AddSingleton<ITextGenerator, LocalTextGenerator>();
}
```

Add the needed `using` lines to `Program.cs` (`IronProw.Core;` for `AddIronProw`, `Microsoft.Extensions.AI;` for `IChatClient`, `Textree.Host.Rag;` if not already imported).

- [ ] **Step 3: Build the host**

Run: `dotnet build src-host/TextreeHost.slnx`
Expected: builds clean (`TreatWarningsAsErrors=true`). Fix any namespace/property mismatch the compiler flags (see the Step 1 note on `LocalSafetyChatClient` namespace and `*Config` property names).

- [ ] **Step 4: Run host tests**

Run: `dotnet test src-host/tests/Textree.Host.Tests`
Expected: PASS. `ChatEndpointTests` stubs `ITextGenerator` via `RemoveAll<ITextGenerator>() + AddSingleton(stub)`, so its override still wins over the new registration. If the DI now fails to build for a non-stubbed test because `IChatClient` isn't registered in a non-BYO test, confirm those tests don't hit the BYO branch (they set no `TEXTREE_BYO_PRESET`).

- [ ] **Step 5: Manual real-call smoke (one provider, if a key is available)**

If you have a test key, run the assembled host against one frontier preset and confirm a `/chat` request streams tokens (this is the only place the real bridge call is exercised — unit tests use lazy factories and never hit the network). If no key is available, note that in the report; the controller will verify end-to-end.

- [ ] **Step 6: Commit**

```bash
git add src-host/src/Textree.Host/Rag/RemoteChatTextGenerator.cs src-host/src/Textree.Host/Program.cs
git commit -m "refactor: source the BYO generator from iron-prow's IChatClient instead of direct IronHive wiring"
```

---

### Task 7: E2E — frontier preset selection (Playwright)

**Files:**
- Test: `e2e/settings.spec.ts` (add to the existing BYO/Advanced tests — grep `Advanced`/`preset`/`byo` there first)

**Interfaces:**
- Consumes: the running app (CDP on 9222) with the new preset UI.

- [ ] **Step 1: Add a preset-prefill test**

Add a test that opens Settings ▸ Advanced, selects the `openai` preset, and asserts the Base URL prefills. Adapt selectors to the file's existing helpers (read its top for `openSettings`/`loadVault` idioms):

```ts
test("selecting the openai preset prefills its base URL", async () => {
  // open Settings ▸ Advanced ▸ enable custom server (reuse the file's existing steps)
  await page.getByRole("button", { name: /^openai$/i }).click();
  await expect(page.getByLabel(/Base URL/i)).toHaveValue("https://api.openai.com/v1");
});

test("selecting anthropic makes the base URL optional (blank is savable)", async () => {
  await page.getByRole("button", { name: /^anthropic$/i }).click();
  await expect(page.getByLabel(/Base URL/i)).toHaveValue("");
  // model required for Save; fill it and assert Save is enabled with a blank base URL
  await page.getByLabel(/Model/i).fill("claude-opus-4-5");
  await expect(page.getByRole("button", { name: /^Save$/i })).toBeEnabled();
});
```

- [ ] **Step 2: Run E2E**

Run (terminal 1): `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; npm run tauri dev`
Run (terminal 2): `npx playwright test e2e/settings.spec.ts --workers=1`
Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add e2e/settings.spec.ts
git commit -m "test: frontier preset selection prefills and anthropic optional base URL"
```

---

## Self-Review Notes

- **Spec coverage:** preset extension (Task 1), presetDefaults/badge/validity (Task 2), Settings UI + wrap + anthropic-optional (Task 3), connection test (Task 4), host bridge adoption incl. csproj swap + mapping + single-provider registration (Tasks 5-6), identity (Global Constraints + UI stays in Advanced), tests at every layer (Tasks 1-2, 4, 5, 7) + host re-wire covered by existing ChatEndpointTests (Task 6). All spec sections mapped.
- **advisor's 5 risks pinned:** (1) LocalSafetyChatClient kept wrapping the injected client — Task 6 Step 1. (2) ResilienceChatClient now active on the frontier path — called out in the iron-prow facts block + architecture. (3) Program.cs → DI with ConfigureTestServices override preserved — Task 6 Step 2/4. (4) injected IChatClient is a singleton, not disposed per request — Task 6 Step 1 comment ("do NOT dispose"). (5) model is a first-class AddIronHive* arg; baseUrl/apiKey in cfg; anthropic BaseUrl unset when blank — Task 5 Step 4.
- **Unverified-and-flagged (not placeholders):** exact `*Config` property names for OpenAI/GoogleAI/OpenAICompatible (Task 5 note), `LocalSafetyChatClient` namespace + `ModelId` redundancy (Task 6 note) — each carries a concrete "confirm against xml docs/clone, do not guess" instruction, not a vague TODO.
- **Type consistency:** `ByoProviderRegistration.Register(builder, preset, baseUrl, apiKey, model)` signature identical across Task 5 (def) and Task 6 (call). `RemoteChatTextGenerator(IChatClient, string)` identical across Task 6 Step 1 (def) and Step 2 (call).
