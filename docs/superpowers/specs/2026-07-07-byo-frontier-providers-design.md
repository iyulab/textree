# BYO Frontier Providers via the iron-prow Bridge — Design

**Date:** 2026-07-07
**Status:** Approved (design), pending implementation plan

## Goal

Let a user point textree's Q&A/chat at a frontier cloud model (OpenAI, Anthropic,
Gemini, Grok) through the existing BYO ("bring your own AI server") flow, in
addition to the current Ollama / GPUStack / custom presets. Crucially, do this by
**adopting the iron-prow substrate's provider bridge** rather than growing
provider-specific wiring inside the textree host.

## Background: the boundary correction

The textree host currently owns provider branching directly. `RemoteChatTextGenerator`
switches on the preset string and constructs an IronHive generator itself
(`GpuStackMessageGenerator` for `gpustack`, `OpenAICompatibleMessageGenerator`
otherwise). Its own comment notes it deliberately skips "the full IronProw.Core
selection/gateway builder … inert for a single always-on provider."

That was fine for two OpenAI-shaped providers. Adding frontier vendors (Anthropic
is a native Messages API, not OpenAI-compatible) would mean piling more
provider-construction knowledge into the consumer app — a substrate violation
("RAG·임베딩·MCP·런타임은 filer-host 차용, 재발명 금지").

**The key finding:** that provider-construction knowledge already lives in iron-prow.
`IronProw.IronHive` (0.2.2) exposes a per-vendor factory for every provider we need:

- `AddIronHiveOpenAI(builder, name, order, model, Action<OpenAIConfig>)`
- `AddIronHiveAnthropic(builder, name, order, model, Action<AnthropicConfig>)`
- `AddIronHiveGoogleAI(builder, name, order, model, Action<GoogleAIConfig>)`
- `AddIronHiveGpuStack(builder, name, order, model, Action<GpuStackConfig>)`
- `AddIronHiveOpenAICompatible(builder, name, order, model, Action<OpenAICompatibleConfig>)`

and its package pulls in `IronHive.Providers.{Anthropic,GoogleAI,OpenAI,OpenAI.Compatible}`
transitively. So the textree host was *bypassing* an existing substrate bridge, not
lacking one.

**Therefore iron-prow itself needs no change** — no cross-repo publish loop. The work
is entirely inside textree: re-wire the host to call the bridge, and extend the
frontend preset list.

## Scope decision: single provider, no gateway

At runtime the host is bound to exactly ONE provider — `TEXTREE_BYO_PRESET` is read
once at startup (`Program.cs`), and switching providers happens by restarting the host
(`restart_ai_host` re-spawns with new env). So we register exactly one provider with
the bridge and use its `IChatClient` directly. We do NOT adopt
`SelectingChatClient` / `IProviderSelector` / `ProviderKind`-based routing — those are
for per-request selection among many live providers, which textree never needs. The
original "gateway inert for a single provider" reasoning still holds.

## Architecture

### Host (`src-host/src/Textree.Host/`)

- **`Textree.Host.csproj`**: replace the direct `IronHive.Providers.OpenAI.Compatible`
  reference with **`IronProw.IronHive`** (which brings all provider packages
  transitively; keep `IronHive.Core` if still needed by other code).
- **`RemoteChatTextGenerator` (or its replacement)**: drop the hand-rolled
  `BuildGenerator()` switch over `GpuStackMessageGenerator` /
  `OpenAICompatibleMessageGenerator`. Instead, map the preset to the matching
  `AddIronHive*` call on an `IronProwBuilder`, register the single provider, and obtain
  its `IChatClient`. The `IChatClient` is wrapped in the same `LocalSafetyChatClient`
  the local path already uses (the bridge yields an `IChatClient` directly, so the
  current `ChatClientAdapter` shim likely disappears).
- **Preset → bridge mapping:**
  - `openai` → `AddIronHiveOpenAI`
  - `anthropic` → `AddIronHiveAnthropic`
  - `gemini` → `AddIronHiveGoogleAI`
  - `gpustack` → `AddIronHiveGpuStack`
  - `grok`, `custom` → `AddIronHiveOpenAICompatible` (pure OpenAI-compatible endpoints)
  - `ollama` → `AddIronHiveOpenAICompatible` (Ollama's `/v1` OpenAI-compatible surface),
    matching today's behavior
- **Config carry-over:** the host still receives `baseUrl` / `apiKey` / `model` from env.
  Each `AddIronHive*` config action sets the fields it needs. Anthropic's `BaseUrl` is
  optional (SDK defaults to api.anthropic.com), so leave it unset when the BYO baseUrl
  is blank.

The exact `IronProwBuilder` entry point (DI extension vs. standalone build) and how the
single `IChatClient` is surfaced to the generator are pinned in the implementation plan
against iron-prow's own usage examples.

### Frontend (`src/lib/`)

- **`byoConfig.ts`**: extend `ByoPreset` to
  `ollama | gpustack | openai | anthropic | gemini | grok | custom`.
- **`settings.helpers.ts`**: `presetDefaults` baseUrl prefills:
  - `openai` → `https://api.openai.com/v1`
  - `grok` → `https://api.x.ai/v1`
  - `gemini` → `https://generativelanguage.googleapis.com/v1beta/openai`
  - `anthropic` → `""` (baseUrl optional; SDK default)
  - existing `ollama` / `gpustack` unchanged; `custom` stays blank
  - `byoProviderBadge` gains labels for the new presets.
- **`Settings.svelte`**: the preset `seg-group` grows to 7 buttons — allow it to wrap to
  two rows (CSS `flex-wrap`), no new component. When `anthropic` is selected, the Base
  URL field is optional (placeholder "leave blank for default"); Model + API key remain
  required. Everything stays inside the existing **Settings ▸ Advanced: custom AI server**
  disclosure.

### Connection test (Rust, `src-tauri/`)

The current `testByoConnection` probes reachability of `baseUrl`. For `anthropic` with a
blank baseUrl, either probe the default Anthropic endpoint or skip the reachability probe
and let the first real `/chat` call surface errors (the same lazy-failure model
`RemoteChatTextGenerator` already documents). The exact behavior is decided in the plan;
the design requirement is only that a blank-baseUrl anthropic config is savable and not
rejected by the client-side `isValidByoUrl` gate.

## Identity compliance

All of this stays inside **Settings ▸ Advanced** and never surfaces on the product face
(README, homepage, welcome). The default remains the free local model; frontier
providers are opt-in BYO, self-keyed, "amplification if present" — consistent with the
local-first north star and the messaging guardrails (no competitor/vendor names promoted
on the product surface).

## Testing

- **Unit (vitest):** `byoConfig` preset round-trip for the new presets;
  `settings.helpers` `presetDefaults` / `byoProviderBadge` for each new preset.
- **Host (dotnet test):** the preset → `AddIronHive*` mapping picks the right bridge call
  per preset (including anthropic and the grok→compatible fallback); a blank-baseUrl
  anthropic config constructs without throwing.
- **E2E (Playwright):** the Advanced preset control shows the new options and selecting
  one prefills the expected base URL; the wrapped layout renders.

## Out of scope

- Any change to iron-prow itself (it already provides the bridge).
- `SelectingChatClient` / multi-provider runtime routing.
- Per-provider model pickers or model auto-discovery (`AnthropicModelFinder` etc.) — the
  user types the model name, as today.
- Promoting frontier providers anywhere on the product surface.
