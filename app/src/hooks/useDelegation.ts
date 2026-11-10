import { useCallback, useEffect, useMemo, useState } from "react";
import { api, subscribe } from "../api/client";
import type { Delegation } from "../api/contract";
import { availableActions } from "../delegation/status";

/** One delegation, live, with the user actions wired to the commands. */
export function useDelegation(id: string | null | undefined) {
  const [delegation, setDelegation] = useState<Delegation | null>(null);
  const [error, setError] = useState<string | null>(null);        // load error
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setDelegation(null);
    setError(null);
    setActionError(null);
    if (!id) return;
    let alive = true;
    let un: (() => void) | null = null;
    api.getDelegation(id).then((d) => alive && setDelegation(d)).catch((e) => alive && setError(String(e)));
    void subscribe.delegation((d) => { if (d.id === id) setDelegation(d); })
      .then((u) => (alive ? (un = u) : u()));
    return () => { alive = false; un?.(); };
  }, [id]);

  const run = useCallback(async (fn: () => Promise<Delegation>) => {
    setBusy(true);
    setActionError(null);
    try {
      const d = await fn();
      setDelegation(d);
      return d;
    } catch (e) {
      setActionError(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      setBusy(false);
    }
  }, []);

  const actions = useMemo(() => ({
    start: () => run(() => api.startDelegation(id!)),
    approve: () => run(() => api.approveStep(id!)),
    adjust: (feedback: string) => run(() => api.adjustStep(id!, feedback)),
    cancel: () => run(() => api.cancelDelegation(id!)),
    finish: (merge: boolean) => run(() => api.finishDelegation(id!, merge)),
  }), [id, run]);

  const can = delegation ? availableActions(delegation) : null;
  return { delegation, error, actionError, busy, actions, can };
}
