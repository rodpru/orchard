export interface RemovalSteps {
  /** Closes the worktree's views and resolves once its terminals have exited. */
  closeSessions: () => Promise<void>;
  /** Asks git to remove the worktree (and, if asked, its branch). */
  remove: () => Promise<void>;
  /** Whether git still lists the worktree, or null when the list could not be read. */
  stillListed: () => Promise<boolean | null>;
  /** Forgets app state tied to the worktree, such as its pin. */
  forget: () => void;
}

/**
 * Removes a worktree without losing its sessions to a removal that never
 * happened. Without force, git may refuse (uncommitted changes, a lock), so
 * the sessions are closed only once the worktree is gone. With force, the
 * user has already given up the work in progress, and closing first stops an
 * agent still writing into the directory from racing the removal.
 *
 * Rethrows the removal's error after cleaning up, so the caller can show it.
 */
export async function removeKeepingSessions(force: boolean, steps: RemovalSteps): Promise<void> {
  if (force) await steps.closeSessions();
  let failure: { error: unknown } | null = null;
  try {
    await steps.remove();
  } catch (error) {
    failure = { error };
  }
  // A failure after git removed the worktree (deleting the branch) still
  // leaves it gone, so the list decides when the removal threw.
  const listed = await steps.stillListed();
  if (!failure || listed === false) {
    if (!force) await steps.closeSessions();
    steps.forget();
  }
  if (failure) throw failure.error;
}
