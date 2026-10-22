import { useEffect } from "react";

/** A window coming back fires focus and visibilitychange together; one poll is enough. */
const REFOCUS_GAP_MS = 3000;

/**
 * Calls `poll` now, every `intervalMs` while the window is visible, and again
 * when the window regains focus or becomes visible. Hidden windows do not
 * poll, a poll never starts while the previous one is still running, and a
 * new `poll` identity (for example a changed repo list) polls at once.
 */
export function usePolling(poll: () => unknown, intervalMs: number, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    let running = false;
    let finished = 0;
    const run = async () => {
      if (document.hidden || running) return;
      running = true;
      try {
        await poll();
      } finally {
        running = false;
        finished = Date.now();
      }
    };
    const refocus = () => {
      if (Date.now() - finished > REFOCUS_GAP_MS) void run();
    };
    void run();
    const timer = window.setInterval(() => void run(), intervalMs);
    window.addEventListener("focus", refocus);
    document.addEventListener("visibilitychange", refocus);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", refocus);
      document.removeEventListener("visibilitychange", refocus);
    };
  }, [poll, intervalMs, enabled]);
}
