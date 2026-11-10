import { useCallback, useEffect, useState } from "react";
import { api, subscribe } from "../api/client";
import type { Delegation } from "../api/contract";
import { isFinished, upsertDelegation } from "../delegation/status";

/** Delegations of a context (or all), live through nyx://delegation. */
export function useDelegations(contextId: string | null | undefined, includeFinished = false) {
  const [delegations, setDelegations] = useState<Delegation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setDelegations(await api.listDelegations(contextId ?? null, includeFinished));
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [contextId, includeFinished]);

  useEffect(() => {
    void refresh();
    let alive = true;
    let un: (() => void) | null = null;
    void subscribe.delegation((d) => {
      if (contextId && d.contextId !== contextId) return;
      setDelegations((list) =>
        !includeFinished && isFinished(d.status) ? list.filter((x) => x.id !== d.id) : upsertDelegation(list, d));
    }).then((u) => (alive ? (un = u) : u()));
    return () => { alive = false; un?.(); };
  }, [refresh, contextId, includeFinished]);

  return { delegations, loading, error, refresh };
}
