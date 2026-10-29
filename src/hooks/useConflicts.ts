import { useCallback, useMemo, useRef, useState } from "react";
import { api, errorMessage } from "../lib/api";
import { withoutKey } from "../lib/collections";
import type { Clash } from "../lib/types";
import { usePolling } from "./usePolling";

const CONFLICT_POLL_MS = 30_000;
const FETCH_EVERY_MS = 5 * 60_000;

/**
 * Checks each repo's worktrees for clashes every 30 s. With `fetchRemote`,
 * a check first fetches `origin` and the open PRs, at most every 5 minutes
 * per repo. A failed check keeps the last known clashes, as PR status does.
 */
export function useConflicts(repos: string[], fetchRemote: boolean) {
  const [byRepo, setByRepo] = useState<Record<string, Partial<Record<string, Clash[]>>>>({});
  const [checkErrors, setCheckErrors] = useState<Record<string, string>>({});
  const [fetchErrors, setFetchErrors] = useState<Record<string, string>>({});
  const lastFetch = useRef<Record<string, number>>({});

  const refreshConflicts = useCallback(async () => {
    await Promise.all(
      repos.map(async (repo) => {
        const fetch = fetchRemote && Date.now() - (lastFetch.current[repo] ?? 0) >= FETCH_EVERY_MS;
        if (fetch) lastFetch.current[repo] = Date.now();
        try {
          const report = await api.conflictsCheck(repo, fetch);
          setByRepo((current) => ({ ...current, [repo]: report.clashes }));
          setCheckErrors((current) => withoutKey(current, repo));
          if (fetch) {
            const error = report.fetchError;
            setFetchErrors((current) => (error ? { ...current, [repo]: error } : withoutKey(current, repo)));
          }
        } catch (e) {
          setCheckErrors((current) => ({ ...current, [repo]: errorMessage(e) }));
        }
      }),
    );
  }, [repos, fetchRemote]);

  usePolling(refreshConflicts, CONFLICT_POLL_MS, repos.length > 0);

  /** Clashes per worktree path; worktrees without any are absent. */
  const clashes = useMemo(() => {
    const result: Record<string, Clash[]> = {};
    for (const repo of repos) {
      for (const [path, list] of Object.entries(byRepo[repo] ?? {})) if (list?.length) result[path] = list;
    }
    return result;
  }, [repos, byRepo]);

  /** A failed check, else a failed fetch, per repo. */
  const conflictErrors = useMemo(
    () => ({ ...(fetchRemote ? fetchErrors : {}), ...checkErrors }),
    [fetchRemote, fetchErrors, checkErrors],
  );

  return { clashes, conflictErrors, refreshConflicts };
}
