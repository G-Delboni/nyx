// Live state for the panel: one snapshot + event subscriptions. No polling.
import { useCallback, useEffect, useRef, useState } from "react";
import { api, subscribe } from "./api/client";
import type { Snapshot } from "./api/contract";

export function useSnapshot() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [eventTick, setEventTick] = useState(0); // bumps on each nyx://event

  const refresh = useCallback(async () => {
    try {
      setSnap(await api.getSnapshot());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

  useEffect(() => {
    void refresh();
    let alive = true;
    const unlistens: Array<() => void> = [];
    const keep = (p: Promise<() => void>) =>
      p.then((u) => (alive ? unlistens.push(u) : u()));
    keep(subscribe.event(() => {
      setEventTick((n) => n + 1);
      void refreshRef.current(); // unread count + status
    }));
    keep(subscribe.usage((weekUsage) =>
      setSnap((s) => (s ? { ...s, weekUsage } : s))));
    keep(subscribe.agents((agents) =>
      setSnap((s) => (s ? { ...s, agents } : s))));
    return () => {
      alive = false;
      unlistens.forEach((u) => u());
    };
  }, [refresh]);

  return { snap, error, refresh, eventTick };
}
