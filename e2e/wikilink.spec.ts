import { test, expect, type Browser, type Page } from "@playwright/test";
import { connectToApp, loadVault, createTempVault, removeTempVault } from "./helpers";

let browser: Browser;
let page: Page;

test.beforeAll(async () => {
  ({ browser, page } = await connectToApp());
});

test.afterAll(async () => {
  await browser?.close();
});

// Links sit on line 3 so the initial cursor (line 1) does not reveal their raw source.
const VAULT = {
  "Alpha.md": "# Alpha\n\nGo to [[Beta]] now.\n",
  "Beta.md": "# Beta\n\nBeta body here.\n",
  "Gamma.md": "# Gamma\n\nLink to [[Missing]] note.\n",
  "Code.md": "# Code\n\ninline `[[Beta]]` literal\n",
  "Bold.md": "# Bold\n\nmix [[Beta **x** y]] end\n",
};

test("resolved wikilink renders as a link widget (not unresolved)", async () => {
  const vault = createTempVault(VAULT);
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Alpha/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    const link = page.locator(".cm-lp-wikilink", { hasText: "Beta" });
    await expect(link).toBeVisible();
    await expect(link).not.toHaveClass(/cm-lp-wikilink-unresolved/);
  } finally {
    removeTempVault(vault);
  }
});

test("unresolved wikilink renders muted", async () => {
  const vault = createTempVault(VAULT);
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Gamma/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    const link = page.locator(".cm-lp-wikilink-unresolved", { hasText: "Missing" });
    await expect(link).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("clicking a wikilink opens the target note", async () => {
  const vault = createTempVault(VAULT);
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Alpha/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    await page.locator(".cm-lp-wikilink", { hasText: "Beta" }).click();
    // The open note follows to Beta.
    await expect(page.locator(".note-name")).toHaveText("Beta");
    await expect(page.locator(".cm-content")).toContainText("Beta body here.");
  } finally {
    removeTempVault(vault);
  }
});

test("backlinks panel lists notes that link here", async () => {
  const vault = createTempVault(VAULT);
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Beta/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    const panel = page.locator(".backlinks");
    await expect(panel).toBeVisible();
    // Alpha links to Beta → it appears as an incoming link.
    await expect(panel.locator(".backlink", { hasText: "Alpha" })).toBeVisible();
  } finally {
    removeTempVault(vault);
  }
});

test("a wikilink inside inline code stays literal (not rendered)", async () => {
  const vault = createTempVault(VAULT);
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Code/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();
    // The only [[Beta]] is inside `code` → no widget is produced.
    await expect(page.locator(".cm-lp-wikilink")).toHaveCount(0);
  } finally {
    removeTempVault(vault);
  }
});

test("markdown inside a wikilink does not break the editor (RangeSet safe)", async () => {
  const vault = createTempVault(VAULT);
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Bold/ }).click();
    // If the overlapping-replace bug regressed, the editor would throw and not render.
    await expect(page.locator(".cm-content")).toBeVisible();
    await expect(page.locator(".cm-lp-wikilink")).toHaveCount(1);
  } finally {
    removeTempVault(vault);
  }
});

test("typing [[ opens autocomplete and picking inserts a wikilink", async () => {
  const vault = createTempVault(VAULT);
  try {
    await loadVault(page, vault);
    await page.getByRole("treeitem", { name: /Gamma/ }).click();
    await expect(page.locator(".cm-content")).toBeVisible();

    // Move to the end of the document and start a new wikilink.
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+End");
    await page.keyboard.type("[[Bet");

    const tooltip = page.locator(".cm-tooltip-autocomplete");
    await expect(tooltip).toBeVisible();
    // CodeMirror splits the label for match-highlighting (<span>Bet</span>a), so assert on the
    // option label's combined text rather than an exact-text node.
    await expect(tooltip.locator(".cm-completionLabel").first()).toContainText("Beta");

    // Accept by clicking the option (deterministic regardless of keyboard-focus timing) → the
    // source now contains a full [[Beta]] link.
    await tooltip.locator(".cm-completionLabel").first().click();
    await expect(page.locator(".cm-content")).toContainText("[[Beta]]");
  } finally {
    removeTempVault(vault);
  }
});
