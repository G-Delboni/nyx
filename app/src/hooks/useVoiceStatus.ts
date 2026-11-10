import { useCallback, useEffect, useState } from "react";
import { api, subscribe } from "../api/client";
import type { VoiceStatus } from "../api/contract";

/** Voice engine status (key, Windows voice, month chars). Refreshes when a line ends. */
export function useVoiceStatus() {
  const [status, setStatus] = useState<VoiceStatus | null>(null);

  const refresh = useCallback(async () => {
    try { setStatus(await api.getVoiceStatus()); } catch { setStatus(null); }
  }, []);

  useEffect(() => {
    void refresh();
    let alive = true;
    let un: (() => void) | null = null;
    void subscribe.speechEnd(() => void refresh()).then((u) => (alive ? (un = u) : u()));
    return () => { alive = false; un?.(); };
  }, [refresh]);

  return { status, refresh };
}
