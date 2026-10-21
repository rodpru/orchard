import { useCallback, useMemo, useState } from "react";
import { api, errorMessage } from "../lib/api";
import { withoutKey } from "../lib/collections";
import type { PrSummary, Worktree } from "../lib/types";
import { usePolling } from "./usePolling";

const PR_POLL_MS = 60_000;

/**
 * One `gh pr list` per repo, matched to worktrees by branch. A failed call
 * keeps the last known PRs and records the error, so a network blip or an
 * expired `gh` login never hides an existing PR.
 */
export function usePrStatus(repos: string[], worktrees: Record<string, Worktree[]>) {
  const [byRepo, setByRepo] = useState<Record<string, Partial<Record<string, PrSummary>>>>({});
  const [prErrors, setPrErrors] = useState<Record<string, string>>({});

  const refreshPrs = useCallback(async () => {
    await Promise.all(
      repos.map(async (repo) => {
        try {
          const prs = await api.prList(repo);
          setByRepo((current) => ({ ...current, [repo]: prs }));
          setPrErrors((current) => withoutKey(current, repo));
        } catch (e) {
          setPrErrors((current) => ({ ...current, [repo]: errorMessage(e) }));
        }
      }),
    );
  }, [repos]);

  usePolling(refreshPrs, PR_POLL_MS, repos.length > 0);

  /** PR per worktree path; `null` when the branch has no PR. */
  const prs = useMemo(() => {
    const result: Record<string, PrSummary | null> = {};
    for (const repo of repos) {
      for (const wt of worktrees[repo] ?? []) {
        if (wt.branch && !wt.isMain) result[wt.path] = byRepo[repo]?.[wt.branch] ?? null;
      }
    }
    return result;
  }, [repos, worktrees, byRepo]);

  return { prs, prErrors, refreshPrs };
}
