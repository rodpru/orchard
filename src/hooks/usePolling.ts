import { useEffect, useRef } from "react";

/** A window coming back fires focus and visibilitychange together; one poll is enough. */
const REFOCUS_GAP_MS = 3000;

/**
 * Calls `poll` now, every `intervalMs` while the window is visible, and again
 * when the window regains focus or becomes visible. Hidden windows do not
 * poll, a poll never starts while the previous one is still running (even one
 * started with an older `poll`), and a new `poll` identity (for example a
 * changed repo list) polls at once.
 */
export function usePolling(poll: () => unknown, intervalMs: number, enabled = true) {
  const running = useRef(false);
  const finished = useRef(0);

  useEffect(() => {
    if (!enabled) return;
    const run = async () => {
      if (document.hidden || running.current) return;
      running.current = true;
      try {
        await poll();
      } finally {
        running.current = false;
        finished.current = Date.now();
      }
    };
    const refocus = () => {
      if (Date.now() - finished.current > REFOCUS_GAP_MS) void run();
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
