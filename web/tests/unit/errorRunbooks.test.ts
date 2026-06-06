import { describe, expect, it } from "vitest";
import {
  errorInlineRemedyKey,
  errorRunbookPath,
  errorRunbookUrl,
} from "../../src/state/errorRunbooks";

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

  it("offers an inline remedy only for ids without a public runbook", () => {
    // Errors that have a runbook link do not also show an inline remedy.
    expect(errorInlineRemedyKey("E-1101")).toBeUndefined();
    expect(errorInlineRemedyKey(undefined)).toBeUndefined();
    // Mapped UI ids get a specific remedy.
    expect(errorInlineRemedyKey("E-UI-0001")).toBe("errors.inline.E-UI-0001");
    // Ids that already have a public runbook do not also show an inline remedy.
    expect(errorInlineRemedyKey("E-4001")).toBeUndefined();
    // Any other unmapped id falls back to a generic remedy so no error is left
    // without guidance.
    expect(errorInlineRemedyKey("E-9999")).toBe("errors.inline.generic");
    expect(errorInlineRemedyKey("E-UI-0010")).toBe("errors.inline.generic");
  });
});