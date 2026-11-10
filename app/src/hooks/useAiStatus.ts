import { useCallback, useEffect, useState } from "react";
import { api, subscribe } from "../api/client";
import type { AiStatus } from "../api/contract";

/** AI availability (API key present) and Nyx's own weekly spend. Refreshes when a chat reply lands. */
export function useAiStatus() {
  const [status, setStatus] = useState<AiStatus | null>(null);

  const refresh = useCallback(async () => {
    try { setStatus(await api.getAiStatus()); } catch { setStatus(null); }
  }, []);

  useEffect(() => {
    void refresh();
    let alive = true;
    let un: (() => void) | null = null;
    void subscribe.chat((m) => { if (m.role === "nyx") void refresh(); })
      .then((u) => (alive ? (un = u) : u()));
    return () => { alive = false; un?.(); };
  }, [refresh]);

  return { status, keyPresent: status?.keyPresent ?? false, refresh };
}
