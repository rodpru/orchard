import { useCallback, useEffect, useRef, useState } from "react";
import { withItem, withoutItem } from "../lib/collections";

/** Activity, unread output and bell state shown as the sidebar status dots. */
export function useSessionIndicators(selected: string | null) {
  const [activity, setActivity] = useState<Record<string, number>>({});
  const [unread, setUnread] = useState<Set<string>>(new Set());
  const [attention, setAttention] = useState<Set<string>>(new Set());
  const selectedRef = useRef(selected);
  useEffect(() => {
    selectedRef.current = selected;
  }, [selected]);

  const onActivity = useCallback((path: string) => {
    setActivity((current) => ({ ...current, [path]: Date.now() }));
    if (selectedRef.current !== path) setUnread((current) => withItem(current, path));
  }, []);

  const onBell = useCallback((path: string) => {
    if (selectedRef.current !== path || !document.hasFocus()) {
      setAttention((current) => withItem(current, path));
    }
  }, []);

  const markSeen = useCallback((path: string) => {
    setUnread((current) => withoutItem(current, path));
    setAttention((current) => withoutItem(current, path));
  }, []);

  return { activity, unread, attention, onActivity, onBell, markSeen };
}
