import { useEffect } from "react";

/**
 * Calls `poll` now, every `intervalMs` while the window is visible, and again
 * when the window regains focus or becomes visible. Hidden windows do not
 * poll. A new `poll` identity (for example a changed repo list) polls at once.
 */
export function usePolling(poll: () => unknown, intervalMs: number, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    const run = () => {
      if (!document.hidden) void poll();
    };
    run();
    const timer = window.setInterval(run, intervalMs);
    window.addEventListener("focus", run);
    document.addEventListener("visibilitychange", run);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", run);
      document.removeEventListener("visibilitychange", run);
    };
  }, [poll, intervalMs, enabled]);
}
