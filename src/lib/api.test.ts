import { describe, expect, it } from "vitest";
import { basename, errorMessage, isOpenableUrl } from "./api";

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

describe("isOpenableUrl", () => {
  it("allows https anywhere and http only to this machine", () => {
    expect(isOpenableUrl("https://github.com/rodpru/orchard")).toBe(true);
    expect(isOpenableUrl("http://localhost:5173/")).toBe(true);
    expect(isOpenableUrl("http://127.0.0.1:8080/api")).toBe(true);
    expect(isOpenableUrl("http://[::1]:3000")).toBe(true);
  });

  it("refuses remote http, look-alike hosts and other schemes", () => {
    expect(isOpenableUrl("http://example.com")).toBe(false);
    expect(isOpenableUrl("http://localhost.evil.com/")).toBe(false);
    expect(isOpenableUrl("file:///etc/passwd")).toBe(false);
    expect(isOpenableUrl("not a url")).toBe(false);
  });
});
