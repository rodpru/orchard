import { describe, expect, it } from "vitest";
import { removeKeepingSessions, type RemovalSteps } from "./removal";

/** Records the order of the steps; `remove` fails with `error` when given. */
function steps(opts: { error?: Error; listed: boolean | null }) {
  const calls: string[] = [];
  const s: RemovalSteps = {
    closeSessions: () => {
      calls.push("close");
      return Promise.resolve();
    },
    remove: () => {
      calls.push("remove");
      return opts.error ? Promise.reject(opts.error) : Promise.resolve();
    },
    stillListed: () => {
      calls.push("list");
      return Promise.resolve(opts.listed);
    },
    forget: () => calls.push("forget"),
  };
  return { s, calls };
}

describe("removeKeepingSessions", () => {
  it("closes the sessions only after a removal without force succeeds", async () => {
    const { s, calls } = steps({ listed: false });
    await removeKeepingSessions(false, s);
    expect(calls).toEqual(["remove", "list", "close", "forget"]);
  });

  it("keeps the sessions and the pin when git refuses the removal", async () => {
    const refused = new Error("contains modified or untracked files");
    const { s, calls } = steps({ error: refused, listed: true });
    await expect(removeKeepingSessions(false, s)).rejects.toBe(refused);
    expect(calls).toEqual(["remove", "list"]);
  });

  it("keeps the sessions when the removal fails and the list cannot be read", async () => {
    const { s, calls } = steps({ error: new Error("git failed"), listed: null });
    await expect(removeKeepingSessions(false, s)).rejects.toThrow("git failed");
    expect(calls).toEqual(["remove", "list"]);
  });

  it("closes the sessions when the worktree is gone but deleting the branch failed", async () => {
    const { s, calls } = steps({ error: new Error("worktree removed, but deleting branch x failed"), listed: false });
    await expect(removeKeepingSessions(false, s)).rejects.toThrow("worktree removed");
    expect(calls).toEqual(["remove", "list", "close", "forget"]);
  });

  it("closes the sessions before a forced removal", async () => {
    const { s, calls } = steps({ listed: false });
    await removeKeepingSessions(true, s);
    expect(calls).toEqual(["close", "remove", "list", "forget"]);
  });

  it("forgets the worktree after a successful removal even when the list cannot be read", async () => {
    const { s, calls } = steps({ listed: null });
    await removeKeepingSessions(false, s);
    expect(calls).toEqual(["remove", "list", "close", "forget"]);
  });
});
