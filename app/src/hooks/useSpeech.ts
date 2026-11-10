// What Nyx is saying right now (TTS). The core emits nyx://speech when audio starts and
// nyx://speech-end when it stops; the envelope drives the avatar's mouth and the portrait's
// speaking indicator. Owner: orchestrator (shared by the scene and the UI).
import { useEffect, useState } from "react";
import type { SpeechEvent } from "../api/contract";
import { subscribe } from "../api/client";

export interface CurrentSpeech {
  speech: SpeechEvent;
  /** performance.now() when the speech event arrived (audio start). */
  startedAt: number;
}

/** Mouth openness 0..1 at time `now` (performance.now()) for the given speech, 0 when silent. */
export function envelopeAt(cur: CurrentSpeech | null, now: number, stepMs = 50): number {
  if (!cur) return 0;
  const i = Math.floor((now - cur.startedAt) / stepMs);
  const e = cur.speech.envelope;
  return i >= 0 && i < e.length ? e[i] : 0;
}

export function useSpeech(): CurrentSpeech | null {
  const [cur, setCur] = useState<CurrentSpeech | null>(null);
  useEffect(() => {
    let alive = true;
    const offs: Array<() => void> = [];
    void subscribe.speech((speech) => { if (alive) setCur({ speech, startedAt: performance.now() }); }).then((u) => offs.push(u));
    void subscribe.speechEnd(({ id }) => { if (alive) setCur((c) => (c && c.speech.id === id ? null : c)); }).then((u) => offs.push(u));
    return () => { alive = false; offs.forEach((u) => u()); };
  }, []);
  return cur;
}
