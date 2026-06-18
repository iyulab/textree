import { describe, expect, it } from "vitest";
import type { TreeNode } from "./ipc";
import { detectSyncConflicts } from "./syncConflict.helpers";

function leaf(name: string, path = `/v/${name}.md`): TreeNode {
  return { name, kind: "leaf", path, body_path: path, children: [] };
}
function folder(name: string, children: TreeNode[], path = `/v/${name}`): TreeNode {
  return { name, kind: "container", path, body_path: null, children };
}

describe("detectSyncConflicts", () => {
  it("detects a Dropbox conflicted copy", () => {
    const nodes = [leaf("notes (John's conflicted copy 2026-06-18)")];
    expect(detectSyncConflicts(nodes)).toEqual([
      {
        path: "/v/notes (John's conflicted copy 2026-06-18).md",
        name: "notes (John's conflicted copy 2026-06-18)",
        source: "dropbox",
      },
    ]);
  });

  it("detects a Syncthing conflict file", () => {
    const nodes = [leaf("notes.sync-conflict-20260618-120000-ABCDEF")];
    expect(detectSyncConflicts(nodes)).toEqual([
      {
        path: "/v/notes.sync-conflict-20260618-120000-ABCDEF.md",
        name: "notes.sync-conflict-20260618-120000-ABCDEF",
        source: "syncthing",
      },
    ]);
  });

  it("matches 'conflicted copy' case-insensitively", () => {
    const nodes = [leaf("plan (Conflicted Copy)")];
    expect(detectSyncConflicts(nodes).map((c) => c.source)).toEqual(["dropbox"]);
  });

  it("recurses into folders and flattens results", () => {
    const nodes = [
      folder("journal", [leaf("day (conflicted copy)", "/v/journal/day (conflicted copy).md")]),
    ];
    expect(detectSyncConflicts(nodes).map((c) => c.path)).toEqual([
      "/v/journal/day (conflicted copy).md",
    ]);
  });

  it("detects a conflicted folder (container), not only leaves", () => {
    const nodes = [folder("project (conflicted copy)", [])];
    expect(detectSyncConflicts(nodes).map((c) => c.source)).toEqual(["dropbox"]);
  });

  it("does NOT flag ambiguous patterns (precision over recall)", () => {
    // OneDrive '-MACHINE', Google ' (1)', iCloud ' 2' are too ambiguous to flag safely.
    const nodes = [
      leaf("report-DESKTOP-ABC123"),
      leaf("photo (1)"),
      leaf("draft 2"),
      leaf("conflict-resolution-notes"), // 'conflict' word alone must not match
    ];
    expect(detectSyncConflicts(nodes)).toEqual([]);
  });

  it("returns an empty array for a clean tree", () => {
    const nodes = [leaf("a"), folder("f", [leaf("b")])];
    expect(detectSyncConflicts(nodes)).toEqual([]);
  });
});
