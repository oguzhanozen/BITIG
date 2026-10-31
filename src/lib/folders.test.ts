import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import { backend, displayError } from "../services/backend";
import { folderNameError } from "./folders";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("folderNameError", () => {
  it.each(["", "   ", "\t\n"])("rejects a blank folder name: %j", (name) => {
    expect(folderNameError(name)).toBe("Enter a folder name.");
  });

  it("accepts 120 characters and rejects 121", () => {
    expect(folderNameError("a".repeat(120))).toBeNull();
    expect(folderNameError("a".repeat(121))).toBe("Enter a folder name of up to 120 characters.");
  });

  it("counts Unicode characters like the backend", () => {
    expect(folderNameError("😀".repeat(120))).toBeNull();
    expect(folderNameError("😀".repeat(121))).toBe("Enter a folder name of up to 120 characters.");
  });

  it("ignores surrounding whitespace like the backend", () => {
    expect(folderNameError(`  ${"a".repeat(120)}  `)).toBeNull();
    expect(folderNameError("  Highlights  ")).toBeNull();
  });
});

describe("download folder creation errors", () => {
  it("displays the existing create_folder conflict message", async () => {
    vi.mocked(invoke).mockRejectedValueOnce({ code: "conflict", message: "internal detail" });
    const error: unknown = await backend.createFolder("Highlights", "football").catch((caught: unknown) => caught);
    expect(displayError(error, "Folder could not be created.")).toBe("A folder with this name already exists here.");
    expect(invoke).toHaveBeenCalledWith("create_folder", { input: { name: "Highlights", parentId: "football" } });
  });
});
