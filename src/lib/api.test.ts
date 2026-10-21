import { describe, expect, it } from "vitest";
import { basename, errorMessage } from "./api";

describe("errorMessage", () => {
  it("reads the message of a backend error", () => {
    expect(errorMessage({ kind: "command", message: "git worktree remove: exit 128: fatal" })).toBe(
      "git worktree remove: exit 128: fatal",
    );
  });

  it("handles thrown errors and plain strings", () => {
    expect(errorMessage(new Error("boom"))).toBe("boom");
    expect(errorMessage("plugin failed")).toBe("plugin failed");
  });
});

describe("basename", () => {
  it("returns the last path segment", () => {
    expect(basename("/code/app.worktrees/feat-x/")).toBe("feat-x");
  });
});
