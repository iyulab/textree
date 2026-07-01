/**
 * E2E — Chat workspace: multi-turn streaming + graceful degradation.
 *
 * Two run profiles (gate by env var — cannot share a single app instance):
 *
 *   HOST PRESENT:   TEXTREE_ASK_E2E=host   + app launched with TEXTREE_HOST_EXE set.
 *   HOST ABSENT:    TEXTREE_ASK_E2E=absent  + app launched WITHOUT TEXTREE_HOST_EXE.
 *   (default)       Both groups skip if the var is unset — prevents accidental noise in
 *                   the normal suite.
 *
 * Tests connect to the already-running Tauri app via CDP (port 9222) exactly as
 * all other E2E specs do. The ChatView only renders when Chat mode is active; every
 * test must open a vault, click a treeitem, and enter Chat mode first.
 *
 * Selectors come from ChatView.svelte:
 *   - Panel section:     section[aria-label="Chat about your notes"]
 *   - Consent button:    button "Enable local AI Q&A"  (text; no aria-label)
 *   - Question input:    aria-label="Question"
 *   - Send button:       aria-label="Send question"    (visible text is "Ask")
 *   - Turn bubbles:      .chat-bubble
 *   - Assistant bubble:  .chat-turn.assistant .chat-bubble
 *   - Citations:         .chat-citation
 *   - Status:            [role="status"]
 *   - Error:             [role="alert"]
 *   - Chat toggle:       Ctrl+Shift+M (global shortcut) or button[aria-label="Switch to chat"] in .title header
 */
import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
  readVaultFile,
} from "./helpers";

// ── Profile detection ────────────────────────────────────────────────────────
const PROFILE = process.env["TEXTREE_ASK_E2E"] ?? "";

// ── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Invoke a Tauri command through the live IPC bridge (__TAURI_INTERNALS__).
 * Works only after connectToApp (guarantees the test bridge is up).
 */
async function tauriInvoke<T>(
  page: Page,
  cmd: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return page.evaluate(
    ([c, a]) =>
      (
        (
          window as unknown as {
            __TAURI_INTERNALS__?: {
              invoke: (cmd: string, args?: unknown) => Promise<unknown>;
            };
          }
        ).__TAURI_INTERNALS__
      )?.invoke(c, a) as Promise<T>,
    [cmd, args] as const,
  ) as Promise<T>;
}

/**
 * Poll host_status via Tauri IPC until status === "ready" AND generatorReady (max timeoutMs).
 * Used by the host-present profile to gate Q&A tests on model readiness.
 *
 * The generator loads lazily — the app only triggers it on the first question (chatStore.run
 * → prepare_generation). This prerequisite sends no question, so it must kick generation off
 * itself once the host is Ready; otherwise generatorReady never becomes true and the gate
 * would time out even on a healthy host.
 */
async function pollHostReady(page: Page, timeoutMs = 300_000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const payload = await tauriInvoke<{ status: string; generatorReady: boolean }>(
      page,
      "host_status",
    ).catch(() => ({ status: "unavailable", generatorReady: false }));
    if (payload.status === "ready" && payload.generatorReady) return;
    if (payload.status === "ready" && !payload.generatorReady) {
      // Kick off the lazy generator load (idempotent; safe to call repeatedly).
      await tauriInvoke(page, "prepare_generation").catch(() => {});
    }
    if (payload.status === "unavailable") {
      throw new Error("Host status became unavailable while waiting for ready");
    }
    await new Promise<void>((r) => setTimeout(r, 2_000));
  }
  throw new Error(`Host did not reach "ready" + generatorReady within ${timeoutMs / 1000}s`);
}

/**
 * Set the generation consent flag in localStorage so the panel skips the
 * consent gate and renders the question input directly.
 */
async function setGenerationConsent(page: Page, value: boolean): Promise<void> {
  await page.evaluate(
    (v) => localStorage.setItem("ai-generation-consent", v ? "true" : "false"),
    value,
  );
}

/**
 * Clear localStorage keys related to AI consent so each test starts clean.
 */
async function clearAiConsent(page: Page): Promise<void> {
  await page.evaluate(() => {
    localStorage.removeItem("ai-generation-consent");
    localStorage.removeItem("ai-consent");
  });
}

// ── Vault fixture — short notes that yield a clear answer about photosynthesis ──

const ASK_VAULT_FILES: Record<string, string> = {
  "photosynthesis.md": [
    "# Photosynthesis",
    "",
    "Photosynthesis is the process by which plants convert sunlight into energy.",
    "Chlorophyll in leaves absorbs light and drives the conversion of CO2 and water into glucose.",
    "Oxygen is released as a by-product during photosynthesis.",
    "",
  ].join("\n"),
  "chlorophyll.md": [
    "# Chlorophyll",
    "",
    "Chlorophyll is the green pigment in plant cells responsible for capturing sunlight.",
    "It is found inside chloroplasts and is essential for photosynthesis.",
    "Without chlorophyll, plants cannot convert light into chemical energy.",
    "",
  ].join("\n"),
  "glucose.md": [
    "# Glucose",
    "",
    "Glucose is the sugar molecule produced by photosynthesis.",
    "Plants use glucose as a primary energy source for growth and metabolism.",
    "Excess glucose is stored as starch.",
    "",
  ].join("\n"),
};

// ── Helper: switch the open note into Chat mode ──────────────────────────────

async function enterChatMode(page: Page): Promise<void> {
  // The content-bar (.mode-btn "Chat") was removed; the canonical entry point is the
  // global Ctrl+Shift+M shortcut (mod+shift+m in commands.ts). The shortcut fires via
  // svelte:window onkeydown regardless of which non-form element has focus — safe to
  // call after any treeitem click or editor interaction (both are non-form elements).
  await page.keyboard.press("Control+Shift+M");
  await expect(page.locator('section[aria-label="Chat about your notes"]')).toBeVisible({
    timeout: 5_000,
  });
}

// ── HOST-PRESENT: multi-turn streaming + citations ───────────────────────────

let browser: Browser;
let page: Page;

test.describe("host-present: chat streams answers, keeps multi-turn history", () => {
  test.skip(PROFILE !== "host", "Set TEXTREE_ASK_E2E=host + launch with TEXTREE_HOST_EXE.");

  test.beforeAll(async () => {
    ({ browser, page } = await connectToApp());
  });

  test.afterAll(async () => {
    await browser?.close();
  });

  test("host reaches ready with generator loaded (prerequisite)", async () => {
    test.setTimeout(5 * 60_000 + 10_000);
    await pollHostReady(page, 5 * 60_000);
    const payload = await tauriInvoke<{ status: string; generatorReady: boolean }>(
      page,
      "host_status",
    );
    expect(payload.status).toBe("ready");
    expect(payload.generatorReady).toBe(true);
  });

  test("chat answers a question and shows citations; a follow-up keeps history", async () => {
    test.setTimeout(5 * 60_000);
    const vault = createTempVault(ASK_VAULT_FILES);
    try {
      await loadVault(page, vault);
      await expect(page.getByRole("treeitem", { name: /photosynthesis/i })).toBeVisible({
        timeout: 10_000,
      });
      await setGenerationConsent(page, true);
      await page.getByRole("treeitem", { name: /photosynthesis/i }).click();
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      await enterChatMode(page);
      const panel = page.locator('section[aria-label="Chat about your notes"]');
      const input = panel.getByRole("textbox", { name: /question/i });
      await expect(input).toBeVisible({ timeout: 5_000 });

      // Turn 1.
      await input.fill("What is photosynthesis?");
      await page.getByRole("button", { name: /Send question/i }).click();
      await expect(panel.locator(".chat-bubble")).not.toHaveCount(0, { timeout: 30_000 });
      // Assistant bubble streams non-trivial text.
      await expect
        .poll(
          async () =>
            (await panel.locator(".chat-turn.assistant .chat-bubble").innerText()).trim().length,
          { timeout: 4 * 60_000 },
        )
        .toBeGreaterThan(10);
      await expect(panel.locator(".chat-citation")).not.toHaveCount(0, { timeout: 30_000 });

      const bubblesAfterTurn1 = await panel.locator(".chat-bubble").count();

      // Turn 2 — a SECOND assistant response must stream in (not just the user bubble),
      // and prior bubbles must be retained (history accumulates, not replaced).
      await input.fill("And what is the by-product?");
      await page.getByRole("button", { name: /Send question/i }).click();
      await expect
        .poll(async () => panel.locator(".chat-turn.assistant .chat-bubble").count(), {
          timeout: 4 * 60_000,
        })
        .toBeGreaterThanOrEqual(2);
      await expect(panel.locator(".chat-bubble").count()).resolves.toBeGreaterThan(
        bubblesAfterTurn1,
      );
    } finally {
      await clearAiConsent(page);
      removeTempVault(vault);
    }
  });

  test("clicking a citation opens that note and returns to Note mode", async () => {
    test.setTimeout(5 * 60_000);
    const vault = createTempVault(ASK_VAULT_FILES);
    try {
      await loadVault(page, vault);
      await expect(page.getByRole("treeitem", { name: /glucose/i })).toBeVisible({
        timeout: 10_000,
      });
      await setGenerationConsent(page, true);
      await page.getByRole("treeitem", { name: /glucose/i }).click();
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      await enterChatMode(page);
      const panel = page.locator('section[aria-label="Chat about your notes"]');
      const input = panel.getByRole("textbox", { name: /question/i });
      await input.fill("What is glucose?");
      await page.getByRole("button", { name: /Send question/i }).click();
      await expect(panel.locator(".chat-citation")).not.toHaveCount(0, { timeout: 4 * 60_000 });

      const first = panel.locator(".chat-citation").first();
      const citedPath = await first.getAttribute("title");
      expect(citedPath).toBeTruthy();
      await first.click();
      // Returns to Note mode → editor visible.
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
      const stem = citedPath!.replace(/\\/g, "/").split("/").pop()!.replace(/\.md$/, "");
      await expect(page.locator(".title")).toContainText(stem, { timeout: 5_000 });
    } finally {
      await clearAiConsent(page);
      removeTempVault(vault);
    }
  });
});

// ── HOST-PRESENT: Save to note (folder summary → new note) ──────────────────

const SAVE_TO_NOTE_VAULT_FILES: Record<string, string> = {
  "garden/soil.md": [
    "# Soil",
    "",
    "Healthy garden soil is loose, dark, and rich in organic matter.",
    "Compost improves soil structure and feeds beneficial microbes.",
    "",
  ].join("\n"),
  "garden/watering.md": [
    "# Watering",
    "",
    "Most vegetables need about an inch of water per week.",
    "Watering deeply and less often encourages stronger root growth.",
    "",
  ].join("\n"),
};

test.describe("host-present: Save to note writes and opens a summary note", () => {
  test.skip(PROFILE !== "host", "Set TEXTREE_ASK_E2E=host + launch with TEXTREE_HOST_EXE.");

  let saveBrowser: Browser;
  let savePage: Page;

  test.beforeAll(async () => {
    ({ browser: saveBrowser, page: savePage } = await connectToApp());
  });

  test.afterAll(async () => {
    await saveBrowser?.close();
  });

  test("Save to note writes the summary to a new note and opens it", async () => {
    // Must comfortably exceed the sum of this test's own inner waits: pollHostReady (up to
    // 5 * 60_000) + the summary-stream expect.poll (up to 4 * 60_000) + the smaller fixed
    // timeouts below (~35s) + margin. Otherwise the outer timeout can fire first and mask a
    // real pass/fail behind a generic "test timeout exceeded".
    test.setTimeout(10 * 60_000);
    const vault = createTempVault(SAVE_TO_NOTE_VAULT_FILES);
    try {
      // Consent must be set before ChatView renders so the scopebar (and Summarize button) shows.
      await setGenerationConsent(savePage, true);
      await loadVault(savePage, vault);
      await expect(savePage.getByRole("treeitem", { name: /garden/i })).toBeVisible({
        timeout: 10_000,
      });

      // Select the folder (container node) so the chat scope pins to it, not to a single file —
      // mirrors folder-table.spec.ts's folder selection (chatScopeFromSelection() in +page.svelte
      // reads selectedNode.kind === "container" into a { kind: 'folder' } scope).
      await savePage.getByRole("treeitem", { name: /garden/i }).click();
      await enterChatMode(savePage);
      const panel = savePage.locator('section[aria-label="Chat about your notes"]');

      // Ensure the local generator is actually loaded before triggering a real summary —
      // otherwise Summarize would just sit in the 'preparing' gate.
      await pollHostReady(savePage, 5 * 60_000);

      const summarizeBtn = panel.getByRole("button", { name: "Summarize this scope" });
      await expect(summarizeBtn).toBeVisible({ timeout: 5_000 });
      await summarizeBtn.click();

      // Assistant summary turn streams in; wait for non-trivial content (same threshold the
      // multi-turn Q&A test above uses) before treating the summary as ready to save.
      await expect
        .poll(
          async () =>
            (await panel.locator(".chat-turn.assistant .chat-bubble").innerText()).trim().length,
          { timeout: 4 * 60_000 },
        )
        .toBeGreaterThan(10);

      const saveBtn = panel.getByRole("button", { name: "Save summary to a new note" });
      await expect(saveBtn).toBeVisible({ timeout: 5_000 });
      await saveBtn.click();

      // Lands in Note mode with the new summary note open: header title + editor visible.
      // The scope is the "garden" folder (chatScopeFromSelection() sets label = the selected
      // container node's name), so createNoteWithContent's target is
      // "garden/Summary of garden.md" (no collision on a fresh temp vault -> no " (1)" suffix).
      await expect(savePage.locator(".title")).toContainText("Summary of", { timeout: 10_000 });
      await expect(savePage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
      // The H1 is always in the initial viewport, so this lightly confirms Note mode rendered
      // the right document without relying on CodeMirror's virtualized viewport for content
      // further down the page.
      await expect(savePage.locator(".cm-content")).toContainText("# Summary of");

      // Assert the real proof of "writes the summary to a new note": the on-disk file. CodeMirror
      // 6 (constructed without viewportMargin: Infinity in src/lib/Editor.svelte) only renders the
      // viewport + margin, and "## Sources" sits at the bottom of a long summary note, so asserting
      // it via .cm-content can fail on a correct implementation once the note is long enough to be
      // virtualized. Reading the file directly is both flake-proof and more on-spec for this test.
      const noteContent = readVaultFile(vault, "garden/Summary of garden.md");
      expect(noteContent).toContain("# Summary of");
      expect(noteContent).toContain("## Sources");
    } finally {
      await clearAiConsent(savePage);
      removeTempVault(vault);
    }
  });
});

// ── HOST-ABSENT: graceful degradation ────────────────────────────────────────

test.describe("host-absent: chat degrades calmly, Note mode stays functional", () => {
  test.skip(
    PROFILE !== "absent",
    "Set TEXTREE_ASK_E2E=absent + launch WITHOUT TEXTREE_HOST_EXE.",
  );

  let absentBrowser: Browser;
  let absentPage: Page;

  test.beforeAll(async () => {
    ({ browser: absentBrowser, page: absentPage } = await connectToApp());
  });

  test.afterAll(async () => {
    await absentBrowser?.close();
  });

  test("host status is unavailable when no host exe is set", async () => {
    const payload = await tauriInvoke<{ status: string; generatorReady: boolean }>(
      absentPage,
      "host_status",
    );
    expect(payload.status).toBe("unavailable");
    expect(payload.generatorReady).toBe(false);
  });

  test("chat shows the consent gate calmly when not consented", async () => {
    const vault = createTempVault({ "lone.md": "# Lone\n\nJust a lone note.\n" });
    try {
      await loadVault(absentPage, vault);
      await clearAiConsent(absentPage);
      await absentPage.getByRole("treeitem", { name: /lone/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
      await enterChatMode(absentPage);
      await expect(
        absentPage.getByRole("button", { name: /Enable local AI Q&A/i }),
      ).toBeVisible({ timeout: 3_000 });
      await expect(absentPage.locator('[role="alert"]')).toHaveCount(0);
    } finally {
      removeTempVault(vault);
    }
  });

  test("chat shows 'preparing' (not a crash) when consented but host absent", async () => {
    const vault = createTempVault({
      "query-target.md": "# Query Target\n\nContent here.\n",
    });
    try {
      await setGenerationConsent(absentPage, true);
      await loadVault(absentPage, vault);
      await absentPage.getByRole("treeitem", { name: /query-target/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
      await enterChatMode(absentPage);
      const panel = absentPage.locator('section[aria-label="Chat about your notes"]');
      const input = panel.getByRole("textbox", { name: /question/i });
      await input.fill("What topics are covered?");
      await absentPage.getByRole("button", { name: /Send question/i }).click();
      const statusMsg = absentPage.locator('[role="status"]');
      await expect(statusMsg).toBeVisible({ timeout: 5_000 });
      expect(await statusMsg.innerText()).toMatch(/preparing|Local AI/i);
      await expect(absentPage.locator('[role="alert"]')).toHaveCount(0);
    } finally {
      await clearAiConsent(absentPage);
      removeTempVault(vault);
    }
  });

  test("Note mode (editor + tree) stays fully functional with host absent", async () => {
    const vault = createTempVault({ "alpha.md": "# Alpha\n\nAlpha content.\n" });
    try {
      await loadVault(absentPage, vault);
      await absentPage.getByRole("treeitem", { name: /alpha/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
      await absentPage.locator(".cm-content").click();
      await absentPage.keyboard.press("Control+End");
      await absentPage.keyboard.type(" host-absent-edit");
      await expect(absentPage.locator(".title")).toBeVisible({ timeout: 3_000 });
    } finally {
      removeTempVault(vault);
    }
  });

  test("Summarize degrades gracefully (preparing state, no crash) without a host", async () => {
    // Consent must be set before ChatView renders so the scopebar (and Summarize button) is shown.
    const vault = createTempVault({ "summary-target.md": "# Summary Target\n\nSome content.\n" });
    try {
      await setGenerationConsent(absentPage, true);
      await loadVault(absentPage, vault);
      await absentPage.getByRole("treeitem", { name: /summary-target/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
      await enterChatMode(absentPage);

      // Summarize button is visible inside the consented scopebar.
      const btn = absentPage.getByRole("button", { name: "Summarize this scope" });
      await expect(btn).toBeVisible({ timeout: 3_000 });
      await btn.click();

      // Host is absent → gate resolves to 'preparing' (status: "unavailable" → resolveGenerationGate).
      // The panel shows a [role="status"] message and no [role="alert"] (graceful, not a crash).
      const statusMsg = absentPage.locator('[role="status"]');
      await expect(statusMsg).toBeVisible({ timeout: 5_000 });
      expect(await statusMsg.innerText()).toMatch(/preparing|Local AI/i);
      await expect(absentPage.locator('[role="alert"]')).toHaveCount(0);

      // The chat panel (section) must still be visible — no navigation away or crash.
      await expect(
        absentPage.locator('section[aria-label="Chat about your notes"]'),
      ).toBeVisible();
    } finally {
      await clearAiConsent(absentPage);
      removeTempVault(vault);
    }
  });
});

// ── HOST-PRESENT: Summarize (human gate) ─────────────────────────────────────
//
// HUMAN GATE (host-present, real model — cannot be automated without a live host + loaded model):
//
// Manual procedure:
//   1. Launch the app with TEXTREE_HOST_EXE set and TEXTREE_ASK_E2E=host.
//   2. Open a vault with multiple notes (a folder with several .md files works best).
//   3. Click the folder in the tree to set it as the Chat scope.
//   4. Enter Chat mode; wait for the model to finish preparing (the status message clears).
//   5. Click "Summarize" (aria-label="Summarize this scope").
//   6. Verify: a synthetic user turn "Summarize "<folder-name>"" appears immediately.
//   7. Verify: an assistant turn streams tokens until 'done'; the bubble contains a coherent summary.
//   8. Verify: citations (.chat-citation) list the notes that contributed context.
//   9. Verify: the "Copy" button (aria-label="Copy summary") copies the assistant text to the clipboard.
//  10. Large folder (e.g. 20+ notes): verify the summary message or citation list shows "N of M notes"
//      truncation notice (budget-concat limit applied).
//
// Expected: no [role="alert"], chat panel stays visible throughout, summary is non-trivial text.
