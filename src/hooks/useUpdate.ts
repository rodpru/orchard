import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";

const RELEASES_URL = "https://github.com/rodpru/orchard/releases/latest";
/** The first check waits for the repos' git and gh calls to settle. */
const FIRST_CHECK_MS = 30_000;
const UPDATE_CHECK_MS = 6 * 60 * 60_000;

/**
 * Checks for a new release shortly after launch and every few hours while
 * enabled. On macOS the backend installs it silently; on Linux `available`
 * holds the version the user installs from the release page.
 */
export function useUpdate(enabled: boolean) {
  const [available, setAvailable] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled) return;
    // Offline or GitHub unreachable: try again at the next check.
    const check = () => void api.updateCheck().then(setAvailable, () => undefined);
    const first = window.setTimeout(check, FIRST_CHECK_MS);
    const timer = window.setInterval(check, UPDATE_CHECK_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(timer);
    };
  }, [enabled]);

  const openReleases = useCallback(() => api.openUrl(RELEASES_URL), []);

  return { available, openReleases };
}
