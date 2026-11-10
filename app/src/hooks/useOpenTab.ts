import { useEffect, useRef } from "react";
import { subscribe, type OpenTab } from "../api/client";

/** Calls `onOpen(tab)` whenever the core emits nyx://open-tab (tray, toast click, chat action). */
export function useOpenTab(onOpen: (tab: OpenTab) => void) {
  const ref = useRef(onOpen);
  ref.current = onOpen;
  useEffect(() => {
    let alive = true;
    let un: (() => void) | null = null;
    void subscribe.openTab((tab) => ref.current(tab)).then((u) => (alive ? (un = u) : u()));
    return () => { alive = false; un?.(); };
  }, []);
}
