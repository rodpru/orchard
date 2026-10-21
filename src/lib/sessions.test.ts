import { describe, expect, it } from "vitest";
import { sessionId, shellNumber } from "./sessions";

describe("session ids", () => {
  it("finds the shell number of a worktree's shell", () => {
    expect(shellNumber(sessionId.shell("/code/app", 3), "/code/app")).toBe(3);
    expect(shellNumber(sessionId.shell("/code/app", 12), "/code/app")).toBe(12);
  });

  it("ignores other sessions and other worktrees", () => {
    expect(shellNumber(sessionId.agent("/code/app"), "/code/app")).toBeNull();
    expect(shellNumber(sessionId.lazygit("/code/app"), "/code/app")).toBeNull();
    // A worktree whose path extends another one's must not match it.
    expect(shellNumber(sessionId.shell("/code/app-2", 1), "/code/app")).toBeNull();
    expect(shellNumber("/code/app::shell::x", "/code/app")).toBeNull();
  });
});
