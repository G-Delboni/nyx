import { useEffect, useState } from "react";
import { subscribe } from "../api/client";
import type { Settings } from "../api/contract";

/** Settings kept current through nyx://settings (any change, including the tray "Mudo"). */
export function useSettingsLive(initial: Settings | undefined): Settings | undefined {
  const [live, setLive] = useState<Settings | undefined>(initial);
  useEffect(() => { setLive(initial); }, [initial]);
  useEffect(() => {
    let alive = true;
    let un: (() => void) | null = null;
    void subscribe.settings((s) => { if (alive) setLive(s); }).then((u) => (alive ? (un = u) : u()));
    return () => { alive = false; un?.(); };
  }, []);
  return live;
}
