import { useEffect, useRef } from "react";
import { api } from "../api/client";
import type { Delegation, Snapshot } from "../api/contract";
import { nyxLine } from "../nyx/line";
import { createOpeningGate } from "../voice/openingGate";

/**
 * Speaks the opening line once per panel opening (mount, or becoming visible after being
 * hidden). Uses the same phrase the scene shows: nyxLine(..., { justOpened: true, now: openedAt }).
 * The core decides whether to actually speak (enabled, mute, DND, opening toggle).
 */
export function useOpeningSpeech(snap: Snapshot | null, delegations: Delegation[], openedAt: Date): void {
  const gate = useRef(createOpeningGate());
  const text = snap ? nyxLine(snap, delegations, { justOpened: true, now: openedAt }) : null;
  const textRef = useRef(text);
  textRef.current = text;

  const fire = () => {
    const t = gate.current.poll(textRef.current);
    if (t) void api.speakOpening(t).catch(() => {});
  };

  useEffect(() => { fire(); }, [text]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onVis = () => {
      if (document.hidden) gate.current.hide();
      else { gate.current.show(); fire(); }
    };
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
}
