import { describe, expect, it } from "vitest";
import type { Folder } from "../types/domain";
import { buildFolderOptions, formatDuration, sanitizeExportName } from "./format";

function folder(id: string, name: string, parentId: string | null = null): Folder {
  return { id, name, parentId, hasContents: false, createdAt: "", updatedAt: "" };
}

describe("buildFolderOptions", () => {
  it("returns no options for an empty list", () => {
    expect(buildFolderOptions([])).toEqual([]);
  });

  it("keeps single-level folder names and IDs", () => {
    expect(buildFolderOptions([
      folder("football", "Football"),
      folder("basketball", "Basketball"),
    ])).toEqual([
      { value: "football", label: "Football" },
      { value: "basketball", label: "Basketball" },
    ]);
  });

  it("visits nested folders depth-first even when children precede parents", () => {
    expect(buildFolderOptions([
      folder("goals", "Goals", "highlights"),
      folder("football", "Football"),
      folder("basketball", "Basketball"),
      folder("highlights", "Highlights", "football"),
      folder("games", "Games", "basketball"),
    ])).toEqual([
      { value: "football", label: "Football" },
      { value: "highlights", label: "Football / Highlights" },
      { value: "goals", label: "Football / Highlights / Goals" },
      { value: "basketball", label: "Basketball" },
      { value: "games", label: "Basketball / Games" },
    ]);
  });

  it("distinguishes identical names under different parents", () => {
    expect(buildFolderOptions([
      folder("football", "Football"),
      folder("basketball", "Basketball"),
      folder("football-highlights", "Highlights", "football"),
      folder("basketball-highlights", "Highlights", "basketball"),
    ])).toEqual([
      { value: "football", label: "Football" },
      { value: "football-highlights", label: "Football / Highlights" },
      { value: "basketball", label: "Basketball" },
      { value: "basketball-highlights", label: "Basketball / Highlights" },
    ]);
  });

  it("preserves the supplied sibling order at every level", () => {
    expect(buildFolderOptions([
      folder("z", "Z"),
      folder("a", "A"),
      folder("z-child", "Z child", "z"),
      folder("a-child", "A child", "z"),
      folder("z-grandchild", "Z grandchild", "z-child"),
      folder("a-grandchild", "A grandchild", "z-child"),
    ])).toEqual([
      { value: "z", label: "Z" },
      { value: "z-child", label: "Z / Z child" },
      { value: "z-grandchild", label: "Z / Z child / Z grandchild" },
      { value: "a-grandchild", label: "Z / Z child / A grandchild" },
      { value: "a-child", label: "Z / A child" },
      { value: "a", label: "A" },
    ]);
  });

  it("keeps folders with missing parents and their descendants selectable", () => {
    expect(buildFolderOptions([
      folder("child", "Child", "orphan"),
      folder("root", "Root"),
      folder("orphan", "Orphan", "missing"),
    ])).toEqual([
      { value: "root", label: "Root" },
      { value: "orphan", label: "Orphan" },
      { value: "child", label: "Orphan / Child" },
    ]);
  });

  it("visits self-referencing folders only once", () => {
    expect(buildFolderOptions([
      folder("self", "Self", "self"),
      folder("child", "Child", "self"),
    ])).toEqual([
      { value: "self", label: "Self" },
      { value: "child", label: "Self / Child" },
    ]);
  });

  it("visits cycles and their descendants once while preserving valid trees", () => {
    expect(buildFolderOptions([
      folder("a", "A", "b"),
      folder("b", "B", "a"),
      folder("child", "Child", "b"),
      folder("root", "Root"),
      folder("valid-child", "Valid child", "root"),
    ])).toEqual([
      { value: "root", label: "Root" },
      { value: "valid-child", label: "Root / Valid child" },
      { value: "a", label: "A" },
      { value: "b", label: "A / B" },
      { value: "child", label: "A / B / Child" },
    ]);
  });

  it("does not change the input list or virtual folder relationships", () => {
    const folders = Object.freeze([
      Object.freeze(folder("child", "Child", "root")),
      Object.freeze(folder("root", "Root")),
    ]);
    const original = structuredClone(folders);
    buildFolderOptions(folders);
    expect(folders).toEqual(original);
  });
});

describe("formatDuration", () => {
  it("formats short and long media durations", () => {
    expect(formatDuration(83_000)).toBe("1:23");
    expect(formatDuration(3_723_000)).toBe("1:02:03");
  });

  it("does not display invalid duration metadata", () => {
    expect(formatDuration(null)).toBe("—");
    expect(formatDuration(-1)).toBe("—");
  });
});

describe("sanitizeExportName", () => {
  it("removes cross-platform reserved filename content", () => {
    expect(sanitizeExportName('Goals: Messi/2026. ')).toBe("Goals_ Messi_2026");
    expect(sanitizeExportName("CON")).toBe("_CON");
    expect(sanitizeExportName("***")).toBe("___");
  });
});
