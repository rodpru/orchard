import { useEffect, useRef } from "react";

/** A window coming back fires focus and visibilitychange together; one poll is enough. */
const REFOCUS_GAP_MS = 3000;

/**
 * Calls `poll` now, every `intervalMs` while the window is visible, and again
 * when the window regains focus or becomes visible. Hidden windows do not
 * poll, and a poll never starts while the previous one is still running (even
 * one started with an older `poll`). A new `poll` identity (for example a
 * changed repo list) polls at once, or as soon as the poll in flight ends.
 */
export function usePolling(poll: () => unknown, intervalMs: number, enabled = true) {
  const running = useRef(false);
  const finished = useRef(0);
  // The first poll of the current effect, queued behind one still in flight.
  const queued = useRef<(() => void) | null>(null);

  useEffect(() => {
    if (!enabled) return;
    const run = async (queue = false) => {
      if (document.hidden) return;
      if (running.current) {
        if (queue) queued.current = () => void run();
        return;
      }
      running.current = true;
      try {
        await poll();
      } finally {
        running.current = false;
        finished.current = Date.now();
        const next = queued.current;
        queued.current = null;
        next?.();
      }
    };
    const refocus = () => {
      if (Date.now() - finished.current > REFOCUS_GAP_MS) void run();
    };
    void run(true);
    const timer = window.setInterval(() => void run(), intervalMs);
    window.addEventListener("focus", refocus);
    document.addEventListener("visibilitychange", refocus);
    return () => {
      queued.current = null;
      window.clearInterval(timer);
      window.removeEventListener("focus", refocus);
      document.removeEventListener("visibilitychange", refocus);
    };
  }, [poll, intervalMs, enabled]);
}
