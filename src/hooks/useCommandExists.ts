import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";

/** One login-shell check per command for the whole app, not one per worktree. */
const checks = new Map<string, Promise<boolean>>();

function check(command: string): Promise<boolean> {
  let result = checks.get(command);
  if (!result) {
    result = api.commandExists(command).catch(() => true);
    checks.set(command, result);
  }
  return result;
}

/**
 * Whether `command` can run in a terminal pane: `null` while checking.
 * `recheck` forgets the cached answer, for after the user installs the tool.
 */
export function useCommandExists(command: string) {
  const [exists, setExists] = useState<{ command: string; value: boolean } | null>(null);

  useEffect(() => {
    let current = true;
    void check(command).then((value) => {
      if (current) setExists({ command, value });
    });
    return () => {
      current = false;
    };
  }, [command]);

  const recheck = useCallback(() => {
    checks.delete(command);
    setExists(null);
    void check(command).then((value) => setExists({ command, value }));
  }, [command]);

  return { exists: exists?.command === command ? exists.value : null, recheck };
}
