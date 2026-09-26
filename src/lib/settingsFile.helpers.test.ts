import { describe, expect, it } from "vitest";
import {
  describeSettingsOnOpen,
  isStringList,
  isStringListMap,
  isViewsMap,
  parseSettings,
} from "./settingsFile.helpers";

describe("parseSettings", () => {
  it("tells a file that is not there from one that cannot be read", () => {
    expect(parseSettings(null, isStringList)).toEqual({ kind: "absent" });
    expect(parseSettings("{half", isStringList)).toEqual({ kind: "damaged" });
    expect(parseSettings('["a.md"]', isStringList)).toEqual({ kind: "ok", value: ["a.md"] });
  });

  it("takes a file that parses into the wrong shape as unreadable, not as the default", () => {
    // A newer release's shape, or a sync tool's half-merged copy: taken as it is, a favorites file
    // that is not a list breaks the screen, and an order map of other things is mixed and rewritten.
    expect(parseSettings('{"favorites":["a.md"]}', isStringList)).toEqual({ kind: "damaged" });
    expect(parseSettings('{"/v":{"sorted":"by name"}}', isStringListMap)).toEqual({ kind: "damaged" });
    expect(parseSettings("[1, 2]", isStringList)).toEqual({ kind: "damaged" });
    expect(parseSettings("null", isStringListMap)).toEqual({ kind: "damaged" });
  });
});

describe("shapes", () => {
  const view = {
    version: 1,
    name: "Open tasks",
    folder: "/v/tasks",
    columns: null,
    sort: { key: "due", dir: "asc" },
    filters: [{ field: "status", op: "equals", value: "open" }],
  };

  it("accepts the saved views the app writes", () => {
    expect(isViewsMap({ "/v/tasks": [view] })).toBe(true);
    expect(isViewsMap({})).toBe(true);
  });

  it("refuses saved views missing what the table needs", () => {
    expect(isViewsMap({ "/v/tasks": [{ ...view, name: 3 }] })).toBe(false);
    expect(isViewsMap({ "/v/tasks": [{ ...view, filters: "status=open" }] })).toBe(false);
    expect(isViewsMap({ "/v/tasks": view })).toBe(false);
  });

  it("a note order is a map of folders to lists of paths", () => {
    expect(isStringListMap({ "/v": ["/v/b.md", "/v/a.md"] })).toBe(true);
    expect(isStringListMap(["/v/a.md"])).toBe(false);
  });
});

describe("describeSettingsOnOpen", () => {
  it("says nothing when the settings read fine", () => {
    expect(describeSettingsOnOpen(false, [])).toBeNull();
  });

  it("names what could not be read and where it was kept", () => {
    expect(describeSettingsOnOpen(false, ["favorites.json.unreadable-1790000000"])).toBe(
      "This folder's favorites couldn't be read, so it starts empty. The unreadable file was kept (favorites.json.unreadable-1790000000).",
    );
    expect(
      describeSettingsOnOpen(false, ["favorites.json.unreadable-1", "order.json.unreadable-1", "views.json.unreadable-1"]),
    ).toMatch(/^This folder's favorites, note order and saved views couldn't be read, so they start empty/);
  });

  it("says settings a newer release saved are kept as they are, whatever else was found", () => {
    expect(describeSettingsOnOpen(true, [])).toMatch(/saved by a newer Textree.*kept as they are/);
  });
});
