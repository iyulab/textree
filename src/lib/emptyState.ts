/*
 * Single source for empty-state copy. Keeping these here (rather than inline in the markup)
 * means the sidebar hint and the main-pane prompt can't drift apart — "unify the empty states".
 */

/** Sidebar tree area when no vault is open. */
export const NO_VAULT_HINT = "No vault is open.";

/** Main pane prompt + CTA when no vault is open. */
export const NO_VAULT_PROMPT = "Open a local Markdown vault to get started.";

/** Main pane when a vault is open but no note is selected. */
export const NO_NOTE_PROMPT = "Select a note to start reading or editing.";
