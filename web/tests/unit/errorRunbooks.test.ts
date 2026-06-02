import { describe, expect, it } from "vitest";
import { errorRunbookPath, errorRunbookUrl } from "../../src/state/errorRunbooks";

describe("error runbook helpers", () => {
  it("returns a local path and URL for known public errors", () => {
    expect(errorRunbookPath("E-1101")).toBe("docs/errors/E-1101.md");
    expect(errorRunbookPath("E-1103")).toBe("docs/errors/E-1103.md");
    expect(errorRunbookUrl("E-1101")).toBe(
      "https://github.com/escargotrapide/tracemux/blob/main/docs/errors/E-1101.md",
    );
  });

  it("does not link unknown or legacy UI error ids", () => {
    expect(errorRunbookPath("E-9999")).toBeUndefined();
    expect(errorRunbookUrl("E-UI-0010")).toBeUndefined();
    expect(errorRunbookPath(undefined)).toBeUndefined();
  });
});