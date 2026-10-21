import { useCallback, useState } from "react";
import { api, errorMessage } from "../lib/api";
import { withoutKey } from "../lib/collections";
import type { Worktree } from "../lib/types";
import { usePolling } from "./usePolling";

const WORKTREE_POLL_MS = 15_000;

export function useWorktrees(repos: string[]) {
  const [worktrees, setWorktrees] = useState<Record<string, Worktree[]>>({});
  const [repoErrors, setRepoErrors] = useState<Record<string, string>>({});

  /** Refreshes one repo; returns its worktrees, or `null` when git failed. */
  const refreshRepo = useCallback(async (repo: string): Promise<Worktree[] | null> => {
    try {
      const list = await api.listWorktrees(repo);
      setWorktrees((current) => ({ ...current, [repo]: list }));
      setRepoErrors((current) => withoutKey(current, repo));
      return list;
    } catch (e) {
      setRepoErrors((current) => ({ ...current, [repo]: errorMessage(e) }));
      return null;
    }
  }, []);

  const refreshAll = useCallback(() => Promise.all(repos.map(refreshRepo)), [repos, refreshRepo]);

  const forgetRepo = useCallback((repo: string) => {
    setWorktrees((current) => withoutKey(current, repo));
    setRepoErrors((current) => withoutKey(current, repo));
  }, []);

  usePolling(refreshAll, WORKTREE_POLL_MS);

  return { worktrees, repoErrors, refreshRepo, refreshAll, forgetRepo };
}
