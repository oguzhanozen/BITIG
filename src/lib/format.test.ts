import { describe, expect, it } from "vitest";
import { formatDuration, sanitizeExportName } from "./format";

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
