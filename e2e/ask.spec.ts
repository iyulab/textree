/**
 * Task 7: E2E — local AI Q&A (ask panel) + graceful degradation.
 *
 * Two run profiles (gate by env var — cannot share a single app instance):
 *
 *   HOST PRESENT:   TEXTREE_ASK_E2E=host   + app launched with TEXTREE_HOST_EXE set.
 *   HOST ABSENT:    TEXTREE_ASK_E2E=absent  + app launched WITHOUT TEXTREE_HOST_EXE.
 *   (default)       Both groups skip if the var is unset — prevents accidental noise in
 *                   the normal suite.
 *
 * Tests connect to the already-running Tauri app via CDP (port 9222) exactly as
 * all other E2E specs do. The AskPanel only renders when a note is open; every
 * test must open a vault + click a treeitem first.
 *
 * Selectors come directly from AskPanel.svelte:
 *   - Panel section:     section[aria-label="Ask about your notes"]
 *   - Consent button:    button "Enable local AI Q&A"
 *   - Question input:    aria-label="Question"
 *   - Status messages:   role="status" (.ask-status)
 *   - Streaming answer:  .ask-answer[aria-live="polite"]
 *   - Citation buttons:  .ask-citation  (one per source note)
 */
import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
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
 * Poll host_status via Tauri IPC until status === "ready" (max timeoutMs).
 * Used by the host-present profile to gate Q&A tests on model readiness.
 */
async function pollHostReady(page: Page, timeoutMs = 300_000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const payload = await tauriInvoke<{ status: string; generatorReady: boolean }>(
      page,
      "host_status",
    ).catch(() => ({ status: "unavailable", generatorReady: false }));
    if (payload.status === "ready" && payload.generatorReady) return;
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

// ── HOST-PRESENT profile ─────────────────────────────────────────────────────

let browser: Browser;
let page: Page;

test.describe("host-present: ask panel streams answers and opens cited notes", () => {
  test.skip(
    PROFILE !== "host",
    "Set TEXTREE_ASK_E2E=host and launch the app with TEXTREE_HOST_EXE to run this profile.",
  );

  test.beforeAll(async () => {
    ({ browser, page } = await connectToApp());
  });

  test.afterAll(async () => {
    await browser?.close();
  });

  test("host reaches ready status with generator loaded (prerequisite)", async () => {
    // Allow up to 5 min for first-run model download + initialization.
    test.setTimeout(5 * 60_000 + 10_000);
    await pollHostReady(page, 5 * 60_000);
    const payload = await tauriInvoke<{ status: string; generatorReady: boolean }>(
      page,
      "host_status",
    );
    expect(payload.status).toBe("ready");
    expect(payload.generatorReady).toBe(true);
  });

  test("ask panel streams an answer and shows citation buttons", async () => {
    // Allow generous time: model is cached but generation at ~15 tok/sec takes real wall time.
    test.setTimeout(5 * 60_000);

    const vault = createTempVault(ASK_VAULT_FILES);
    try {
      await loadVault(page, vault);

      // Wait for tree to populate.
      await expect(page.getByRole("treeitem", { name: /photosynthesis/i })).toBeVisible({
        timeout: 10_000,
      });

      // Open a note so the AskPanel renders.
      await page.getByRole("treeitem", { name: /photosynthesis/i }).click();
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      const panel = page.locator('section[aria-label="Ask about your notes"]');
      await expect(panel).toBeVisible({ timeout: 5_000 });

      // Grant generation consent so the panel shows the question input.
      await setGenerationConsent(page, true);
      // Reload the panel state by re-clicking the note (consent read on mount reactively).
      await page.getByRole("treeitem", { name: /chlorophyll/i }).click();
      await page.getByRole("treeitem", { name: /photosynthesis/i }).click();
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      // The question input must now be visible (consented state).
      // Use getByRole("textbox") scoped to the panel to avoid strict-mode collision with the
      // "Submit question" button whose aria-label also contains the substring "question".
      const questionInput = panel.getByRole("textbox", { name: /question/i });
      await expect(questionInput).toBeVisible({ timeout: 5_000 });

      // Type a question clearly answerable from the vault.
      await questionInput.fill("What is photosynthesis and what does chlorophyll do?");
      await page.getByRole("button", { name: /Submit question/i }).click();

      // Wait for the streaming answer to appear (generous timeout for generation).
      const answerDiv = page.locator(".ask-answer");
      await expect(answerDiv).not.toBeEmpty({ timeout: 4 * 60_000 });

      // Verify the answer text is non-trivial.
      const answerText = await answerDiv.innerText();
      expect(answerText.trim().length).toBeGreaterThan(10);

      // At least one citation must be shown in the Sources section.
      const citations = page.locator(".ask-citation");
      await expect(citations).not.toHaveCount(0, { timeout: 30_000 });
      const citationCount = await citations.count();
      expect(citationCount).toBeGreaterThanOrEqual(1);
    } finally {
      await clearAiConsent(page);
      removeTempVault(vault);
    }
  });

  test("clicking a citation opens that note in the editor", async () => {
    // Build on the pattern above but in a fresh vault so citations are predictable.
    test.setTimeout(5 * 60_000);

    const vault = createTempVault(ASK_VAULT_FILES);
    try {
      await loadVault(page, vault);
      await expect(page.getByRole("treeitem", { name: /photosynthesis/i })).toBeVisible({
        timeout: 10_000,
      });

      // Open photosynthesis note and grant consent.
      await page.getByRole("treeitem", { name: /photosynthesis/i }).click();
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
      await setGenerationConsent(page, true);

      // Re-open the note to pick up the consent state change.
      await page.getByRole("treeitem", { name: /glucose/i }).click();
      await page.getByRole("treeitem", { name: /photosynthesis/i }).click();
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      const panel = page.locator('section[aria-label="Ask about your notes"]');
      const questionInput = panel.getByRole("textbox", { name: /question/i });
      await expect(questionInput).toBeVisible({ timeout: 5_000 });

      await questionInput.fill("What is glucose and how is it produced?");
      await page.getByRole("button", { name: /Submit question/i }).click();

      // Wait for answer + citations.
      await expect(page.locator(".ask-answer")).not.toBeEmpty({ timeout: 4 * 60_000 });
      const citations = page.locator(".ask-citation");
      await expect(citations).not.toHaveCount(0, { timeout: 30_000 });

      // Click the first citation → the editor must open the CITED note (not just any note).
      const firstCitation = citations.first();
      const citedPath = await firstCitation.getAttribute("title");
      expect(citedPath).toBeTruthy();
      await firstCitation.click();
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
      // Derive the note stem from the cited path and assert the editor title shows it.
      // This fails if the click opened nothing or the wrong note.
      const expectedStem = citedPath!.replace(/\\/g, "/").split("/").pop()!.replace(/\.md$/, "");
      await expect(page.locator(".title")).toContainText(expectedStem, { timeout: 5_000 });
    } finally {
      await clearAiConsent(page);
      removeTempVault(vault);
    }
  });
});

// ── HOST-ABSENT / GRACEFUL-DEGRADATION profile ───────────────────────────────

test.describe("host-absent: graceful degradation when local AI host is unavailable", () => {
  test.skip(
    PROFILE !== "absent",
    "Set TEXTREE_ASK_E2E=absent and launch the app WITHOUT TEXTREE_HOST_EXE to run this profile.",
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

  test("ask panel shows consent button calmly (no crash) when not consented", async () => {
    // IMPORTANT: this test runs BEFORE any consent is set, so the panel shows the gate.
    const vault = createTempVault({
      "lone.md": "# Lone\n\nJust a lone note.\n",
    });
    try {
      await loadVault(absentPage, vault);

      // Clear any residual consent from a previous run.
      await clearAiConsent(absentPage);

      // Open the note so AskPanel renders.
      await absentPage.getByRole("treeitem", { name: /lone/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      const panel = absentPage.locator('section[aria-label="Ask about your notes"]');
      await expect(panel).toBeVisible({ timeout: 5_000 });

      // The consent gate must be shown calmly — "Enable local AI Q&A" button is present.
      const enableBtn = absentPage.getByRole("button", { name: /Enable local AI Q&A/i });
      await expect(enableBtn).toBeVisible({ timeout: 3_000 });

      // No error state visible — the consent view shows no role="alert" elements.
      await expect(absentPage.locator('[role="alert"]')).toHaveCount(0);
    } finally {
      removeTempVault(vault);
    }
  });

  test("ask panel shows 'preparing' status (not a crash/error) when consented but host absent", async () => {
    // Strategy: set consent in localStorage FIRST, then load the vault so the component
    // mounts with consent = true (getGenerationConsent() is called at mount time via $state).
    // Do NOT call page.reload() — that closes the CDP connection.
    const vault = createTempVault({
      "query-target.md": "# Query Target\n\nContent about a specific topic here.\n",
    });
    try {
      // Grant consent BEFORE opening the vault (component mounts after loadVault + tree click).
      await setGenerationConsent(absentPage, true);

      await loadVault(absentPage, vault);
      await expect(absentPage.getByRole("treeitem", { name: /query-target/i })).toBeVisible({
        timeout: 5_000,
      });

      // Open the note — AskPanel mounts here with consent already in localStorage.
      await absentPage.getByRole("treeitem", { name: /query-target/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      const panel = absentPage.locator('section[aria-label="Ask about your notes"]');
      await expect(panel).toBeVisible({ timeout: 5_000 });

      // With consent set, the question input should be visible (consented rendering path).
      // Use getByRole("textbox") scoped to the panel to avoid strict-mode collision with the
      // "Submit question" button whose aria-label also contains the substring "question".
      const questionInput = panel.getByRole("textbox", { name: /question/i });
      await expect(questionInput).toBeVisible({ timeout: 3_000 });

      // Submit a question; host is absent so askStore sees status !== 'ready' → sets 'preparing'.
      await questionInput.fill("What topics are covered in these notes?");
      await absentPage.getByRole("button", { name: /Submit question/i }).click();

      // Expect 'preparing' status message (NOT an error/alert) — calm degradation.
      const statusMsg = absentPage.locator('[role="status"]');
      await expect(statusMsg).toBeVisible({ timeout: 5_000 });
      const text = await statusMsg.innerText();
      expect(text).toMatch(/preparing|Local AI/i);

      // No error alert must appear (the panel handles this gracefully, not as an error).
      await expect(absentPage.locator('[role="alert"]')).toHaveCount(0);
    } finally {
      await clearAiConsent(absentPage);
      removeTempVault(vault);
    }
  });

  test("tree and editor remain fully functional when host is absent", async () => {
    const vault = createTempVault({
      "alpha.md": "# Alpha\n\nAlpha content here.\n",
      "beta.md": "# Beta\n\nBeta content here.\n",
    });
    try {
      await loadVault(absentPage, vault);

      // Tree renders notes.
      await expect(absentPage.getByRole("treeitem", { name: /alpha/i })).toBeVisible({
        timeout: 5_000,
      });
      await expect(absentPage.getByRole("treeitem", { name: /beta/i })).toBeVisible({
        timeout: 5_000,
      });

      // Click a note — editor opens and stays responsive.
      await absentPage.getByRole("treeitem", { name: /alpha/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      // Type in the editor — no hang, no error.
      await absentPage.locator(".cm-content").click();
      await absentPage.keyboard.press("Control+End");
      await absentPage.keyboard.type(" host-absent-edit");

      // Title bar reflects the note name (sanity: app didn't crash).
      await expect(absentPage.locator(".title")).toBeVisible({ timeout: 3_000 });
    } finally {
      removeTempVault(vault);
    }
  });

  test("tantivy full-text search (/) still works when host is absent", async () => {
    const TOKEN = "ask_degradation_unique_x7k3";
    const vault = createTempVault({
      "note-a.md": `# Note A\n\n${TOKEN} is a body-search marker.\n`,
      "note-b.md": "# Note B\n\nUnrelated content.\n",
    });
    try {
      await loadVault(absentPage, vault);

      await absentPage.keyboard.press("Control+p");
      const input = absentPage.getByTestId("palette-input");
      await expect(input).toBeVisible();
      await input.fill(`/${TOKEN}`);

      // tantivy index builds in background — retry until results appear.
      const items = absentPage.getByTestId("palette-item");
      await expect(items.first()).toBeVisible({ timeout: 15_000 });
      await expect(items).toHaveCount(1);
      await expect(items.first()).toContainText(TOKEN);

      await absentPage.keyboard.press("Escape");
    } finally {
      removeTempVault(vault);
    }
  });
});
