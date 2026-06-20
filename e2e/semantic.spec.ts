/**
 * Task 9: E2E — semantic search + graceful degradation.
 *
 * Two run profiles (gate by env var — cannot share a single app instance):
 *
 *   HOST PRESENT:   TEXTREE_SEMANTIC_E2E=host   + app launched with TEXTREE_HOST_EXE set.
 *   HOST ABSENT:    TEXTREE_SEMANTIC_E2E=absent  + app launched WITHOUT TEXTREE_HOST_EXE.
 *   (default)       Both groups skip if the var is unset — prevents accidental noise in
 *                   the normal suite.
 *
 * Tests connect to the already-running Tauri app via CDP (port 9222) exactly as
 * all other E2E specs do.
 */
import { test, expect, type Browser, type Page } from "@playwright/test";
import {
  connectToApp,
  loadVault,
  createTempVault,
  removeTempVault,
} from "./helpers";

// ── Profile detection ────────────────────────────────────────────────────────
const PROFILE = process.env["TEXTREE_SEMANTIC_E2E"] ?? "";

// ── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Invoke a Tauri command through the live IPC bridge (__TAURI_INTERNALS__).
 * Works only when the test bridge (__textreeTest) is up, i.e. after connectToApp.
 */
async function tauriInvoke<T>(page: Page, cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  return page.evaluate(
    ([c, a]) =>
      (
        (window as unknown as { __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: unknown) => Promise<unknown> } })
          .__TAURI_INTERNALS__
      )?.invoke(c, a) as Promise<T>,
    [cmd, args] as const,
  ) as Promise<T>;
}

/** Poll hostStatus via Tauri IPC until "ready" (max timeoutMs). */
async function pollHostReady(page: Page, timeoutMs = 300_000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const status = await tauriInvoke<string>(page, "host_status").catch(() => "unavailable");
    if (status === "ready") return;
    if (status === "unavailable") {
      throw new Error("Host status became unavailable while waiting for ready");
    }
    await new Promise<void>((r) => setTimeout(r, 2_000));
  }
  throw new Error(`Host did not reach "ready" within ${timeoutMs / 1000}s`);
}

/** Dismiss the palette overlay if it is open (cleanup between retry iterations). */
async function dismissPaletteIfOpen(page: Page): Promise<void> {
  const overlay = page.getByTestId("palette-overlay");
  if (await overlay.count() > 0) {
    await page.keyboard.press("Escape");
    await expect(overlay).toHaveCount(0, { timeout: 3_000 }).catch(() => undefined);
  }
}

// ── HOST-PRESENT profile ─────────────────────────────────────────────────────

let browser: Browser;
let page: Page;

// Thematically related notes about nature / ecology (short, semantically cohesive).
const SEMANTIC_VAULT_FILES: Record<string, string> = {
  "forest.md": [
    "# Forest",
    "",
    "Forests are complex ecosystems dominated by trees.",
    "They provide habitat for countless animals and plants.",
    "The canopy layer captures sunlight and drives photosynthesis.",
    "",
  ].join("\n"),
  "ecosystem.md": [
    "# Ecosystem",
    "",
    "An ecosystem is a community of living organisms interacting with their environment.",
    "Energy flows through ecosystems via food webs.",
    "Biodiversity makes ecosystems resilient against disturbance.",
    "",
  ].join("\n"),
  "biodiversity.md": [
    "# Biodiversity",
    "",
    "Biodiversity refers to the variety of life on Earth.",
    "Forests harbor more than half of all terrestrial species.",
    "Conservation of biodiversity protects ecosystem services.",
    "",
  ].join("\n"),
};

test.describe("host-present: semantic search wired end-to-end", () => {
  test.skip(
    PROFILE !== "host",
    "Set TEXTREE_SEMANTIC_E2E=host and launch the app with TEXTREE_HOST_EXE to run this profile.",
  );

  test.beforeAll(async () => {
    ({ browser, page } = await connectToApp());
  });

  test.afterAll(async () => {
    await browser?.close();
  });

  test("host reaches ready status (prerequisite for semantic tests)", async () => {
    // Override timeout: allow up to 5 minutes for first-run model download + initialization.
    test.setTimeout(5 * 60_000 + 10_000);
    await pollHostReady(page, 5 * 60_000);
    const status = await tauriInvoke<string>(page, "host_status");
    expect(status).toBe("ready");
  });

  test("semantic palette returns results and navigates to the first hit", async () => {
    // Override timeout: up to 3 min for reindex + embedding on vault load.
    test.setTimeout(3 * 60_000 + 30_000);
    const vault = createTempVault(SEMANTIC_VAULT_FILES);
    try {
      // Load the vault — open_vault fires reindex_vault in background (host is now Ready).
      await loadVault(page, vault);

      // Confirm the tree seeded correctly.
      await expect(page.getByRole("treeitem", { name: /forest/i })).toBeVisible({ timeout: 5_000 });
      await expect(page.getByRole("treeitem", { name: /ecosystem/i })).toBeVisible({ timeout: 5_000 });
      await expect(page.getByRole("treeitem", { name: /biodiversity/i })).toBeVisible({ timeout: 5_000 });

      const input = page.getByTestId("palette-input");
      const items = page.getByTestId("palette-item");

      // Retry loop: re-issue the '?ecosystem' query until at least one hit appears.
      // This absorbs both indexing lag and embedding lag (the advisor's key pattern).
      // Each iteration: dismiss any open palette, open fresh, query, wait for result.
      await expect(async () => {
        await dismissPaletteIfOpen(page);
        await page.keyboard.press("Control+p");
        await expect(page.getByTestId("palette-overlay")).toBeVisible({ timeout: 5_000 });
        await input.fill("?ecosystem");

        // The ai-unavailable row must NOT appear (would mean host dropped — fail fast).
        const unavailableRow = page.locator(".ai-unavailable");

        // Give a brief window for the in-flight query to settle.
        await expect(items.first()).toBeVisible({ timeout: 8_000 });
        // Confirm it's NOT the unavailable row masquerading as a hit.
        const unavailableCount = await unavailableRow.count();
        expect(unavailableCount).toBe(0);

        await page.keyboard.press("Escape");
        await expect(page.getByTestId("palette-overlay")).toHaveCount(0, { timeout: 3_000 });
      }).toPass({ timeout: 3 * 60_000 });

      // Now reliably open the palette and navigate.
      await page.keyboard.press("Control+p");
      await expect(page.getByTestId("palette-overlay")).toBeVisible();
      await input.fill("?ecosystem");
      await expect(items.first()).toBeVisible({ timeout: 15_000 });

      // Count results (should be > 0, at most 3 notes in vault).
      const hitCount = await items.count();
      expect(hitCount).toBeGreaterThan(0);

      // Enter → navigate to the first hit.
      await page.keyboard.press("Enter");
      await expect(page.getByTestId("palette-overlay")).toHaveCount(0);

      // Editor is open (some note loaded).
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
    } finally {
      await dismissPaletteIfOpen(page);
      removeTempVault(vault);
    }
  });

  test("related notes panel shows other notes excluding the open note itself", async () => {
    // Override timeout: up to 3 min for reindex + embedding.
    test.setTimeout(3 * 60_000 + 30_000);
    const vault = createTempVault(SEMANTIC_VAULT_FILES);
    try {
      await loadVault(page, vault);
      await expect(page.getByRole("treeitem", { name: /forest/i })).toBeVisible({ timeout: 5_000 });

      // Open "forest.md" — we test self-exclusion on this note.
      await page.getByRole("treeitem", { name: /forest/i }).click();
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      // The Related notes section may take time to populate (embedding + similarity).
      // Retry: click ecosystem → forest to force the relatedNotes $effect to re-run on each pass.
      const relatedSection = page.locator('[aria-label="Related notes"]');
      const relatedItems = page.locator('[aria-label="Related notes"] button.related-item');

      await expect(async () => {
        // Re-navigate away then back to force the relatedNotes $effect to re-run.
        await page.getByRole("treeitem", { name: /ecosystem/i }).click();
        await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
        await page.getByRole("treeitem", { name: /forest/i }).click();
        await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

        // Panel must be visible and contain at least one note.
        await expect(relatedSection).toBeVisible({ timeout: 8_000 });
        const count = await relatedItems.count();
        expect(count).toBeGreaterThan(0);
      }).toPass({ timeout: 3 * 60_000 });

      // --- Self-exclusion verification (the critical check carried from prior reviews) ---
      // "forest" must NOT appear in its own Related notes panel.
      // (A note is maximally self-similar; the exclusion works iff host path form == toRelative(activePath).)
      const relatedTexts = await relatedItems.allTextContents();
      const selfPresent = relatedTexts.some((t) => t.trim().toLowerCase() === "forest");
      expect(selfPresent).toBe(false);

      // Panel must contain at least one of the OTHER notes (non-empty + self-absent pair).
      const othersPresent = relatedTexts.some(
        (t) => ["ecosystem", "biodiversity"].includes(t.trim().toLowerCase()),
      );
      expect(othersPresent).toBe(true);
    } finally {
      removeTempVault(vault);
    }
  });

  test("semantic palette click navigation: click a hit (not Enter) opens the note", async () => {
    // Override timeout: up to 3 min for reindex + embedding.
    test.setTimeout(3 * 60_000 + 30_000);
    const vault = createTempVault(SEMANTIC_VAULT_FILES);
    try {
      await loadVault(page, vault);
      await expect(page.getByRole("treeitem", { name: /forest/i })).toBeVisible({ timeout: 5_000 });

      await expect(async () => {
        await dismissPaletteIfOpen(page);
        await page.keyboard.press("Control+p");
        await expect(page.getByTestId("palette-overlay")).toBeVisible({ timeout: 5_000 });
        await page.getByTestId("palette-input").fill("?biodiversity");
        await expect(page.getByTestId("palette-item").first()).toBeVisible({ timeout: 8_000 });
        await page.keyboard.press("Escape");
        await expect(page.getByTestId("palette-overlay")).toHaveCount(0, { timeout: 3_000 });
      }).toPass({ timeout: 3 * 60_000 });

      await page.keyboard.press("Control+p");
      await page.getByTestId("palette-input").fill("?biodiversity");
      const firstHit = page.getByTestId("palette-item").first();
      await expect(firstHit).toBeVisible({ timeout: 15_000 });
      await firstHit.click({ force: true });

      await expect(page.getByTestId("palette-overlay")).toHaveCount(0);
      await expect(page.locator(".cm-content")).toBeVisible({ timeout: 5_000 });
    } finally {
      await dismissPaletteIfOpen(page);
      removeTempVault(vault);
    }
  });
});

// ── HOST-ABSENT / GRACEFUL-DEGRADATION profile ───────────────────────────────

test.describe("host-absent: graceful degradation when host is unavailable", () => {
  test.skip(
    PROFILE !== "absent",
    "Set TEXTREE_SEMANTIC_E2E=absent and launch the app WITHOUT TEXTREE_HOST_EXE to run this profile.",
  );

  let absentBrowser: Browser;
  let absentPage: Page;

  test.beforeAll(async () => {
    ({ browser: absentBrowser, page: absentPage } = await connectToApp());
  });

  test.afterAll(async () => {
    await absentBrowser?.close();
  });

  test("host status is unavailable when host is absent", async () => {
    // Without TEXTREE_HOST_EXE the handle never starts; status must be unavailable immediately.
    const status = await tauriInvoke<string>(absentPage, "host_status");
    expect(status).toBe("unavailable");
  });

  test("tantivy body search (/) still works when host is absent", async () => {
    const TOKEN = "degradation_unique_token_x9z7";
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

      // tantivy index builds in background — auto-retry until results appear.
      const items = absentPage.getByTestId("palette-item");
      await expect(items.first()).toBeVisible({ timeout: 15_000 });
      await expect(items).toHaveCount(1);
      await expect(items.first()).toContainText(TOKEN);

      await absentPage.keyboard.press("Escape");
    } finally {
      removeTempVault(vault);
    }
  });

  test("semantic mode (?) shows muted unavailable row, NOT an error", async () => {
    const vault = createTempVault({ "anything.md": "# Anything\n\nSome content.\n" });
    try {
      await loadVault(absentPage, vault);

      await absentPage.keyboard.press("Control+p");
      const input = absentPage.getByTestId("palette-input");
      await expect(input).toBeVisible();

      // Type a semantic query.
      await input.fill("?forest ecology");

      // The palette must show the AI-unavailable status row (not an error, not a crash).
      // The row has role="status" and class="ai-unavailable" but no data-testid —
      // select by text content (either variant: starting or unavailable).
      const unavailRow = absentPage.locator('.ai-unavailable[role="status"]');
      await expect(unavailRow).toBeVisible({ timeout: 10_000 });

      // The text must be the calm muted message, NOT anything alarming or error-like.
      const text = await unavailRow.innerText();
      expect(text.trim()).toMatch(/^Local AI (is indexing…|is unavailable)$/);

      // No regular palette result items should appear in semantic mode with no host.
      await expect(absentPage.getByTestId("palette-item")).toHaveCount(0);

      await absentPage.keyboard.press("Escape");
    } finally {
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
      await expect(absentPage.getByRole("treeitem", { name: /alpha/i })).toBeVisible({ timeout: 5_000 });
      await expect(absentPage.getByRole("treeitem", { name: /beta/i })).toBeVisible({ timeout: 5_000 });

      // Click a note — editor opens.
      await absentPage.getByRole("treeitem", { name: /alpha/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      // Type in the editor — no hang, no error.
      await absentPage.locator(".cm-content").click();
      await absentPage.keyboard.press("Control+End");
      await absentPage.keyboard.type(" host-absent-edit");

      // The title bar reflects the note name (sanity check: app didn't crash).
      await expect(absentPage.locator(".title")).toBeVisible({ timeout: 3_000 });
    } finally {
      removeTempVault(vault);
    }
  });

  test("related notes panel is silently empty when host is absent", async () => {
    const vault = createTempVault({ "lone.md": "# Lone note\n\nSome content.\n" });
    try {
      await loadVault(absentPage, vault);
      await absentPage.getByRole("treeitem", { name: /lone/i }).click();
      await expect(absentPage.locator(".cm-content")).toBeVisible({ timeout: 5_000 });

      // The Related notes section must NOT appear (empty list → {#if related.length} hides it).
      // Wait 3s for the IPC to resolve before asserting absence.
      await new Promise<void>((r) => setTimeout(r, 3_000));
      await expect(absentPage.locator('[aria-label="Related notes"]')).toHaveCount(0);
    } finally {
      removeTempVault(vault);
    }
  });
});
