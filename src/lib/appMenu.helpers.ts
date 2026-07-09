/*
 * App-menu grouping — pure. Groups the command list into ordered categories for
 * the ⋮ dropdown. No runes/DOM, so vitest covers it directly.
 */
import { type Command, MENU_CATEGORIES, activeCommands } from "./commands";

export interface CommandGroup {
  category: string;
  items: Command[];
}

/**
 * Filters to currently-active commands (when() === true), buckets them by
 * `category`, drops empty categories, and orders groups by MENU_CATEGORIES.
 * Commands whose category is not in MENU_CATEGORIES are omitted (the
 * commands.test.ts guard prevents that in production).
 */
export function groupCommands(commands: Command[]): CommandGroup[] {
  const active = activeCommands(commands);
  return MENU_CATEGORIES.map((category) => ({
    category,
    items: active.filter((c) => c.category === category),
  })).filter((g) => g.items.length > 0);
}
