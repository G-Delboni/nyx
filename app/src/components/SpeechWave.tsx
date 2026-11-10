import { useEffect, useRef, useState } from "react";
import type { CurrentSpeech } from "../hooks/useSpeech";
import { waveLevels } from "../scene/speech";

const BARS = 5;
const MIN = 0.25;              // a bar never vanishes: at silence the glyph is a thin dotted line
const TICK_MS = 50;            // the envelope has one value per 50 ms; drawing faster adds nothing
const STATIC = [0.45, 0.75, 1, 0.75, 0.45];

export interface SpeechWaveProps {
  speech: CurrentSpeech | null;
}

/**
 * Five neon bars beside Nyx's line that follow the voice envelope while she speaks (direct style writes at 20 Hz,
 * no React state per frame) and fade out 300 ms after the audio ends. With reduced motion the bars stand still.
 */
export function SpeechWave({ speech }: SpeechWaveProps) {
  const root = useRef<HTMLSpanElement>(null);
  const [phase, setPhase] = useState<"off" | "on" | "fading">(speech ? "on" : "off");

  useEffect(() => {
    if (speech) { setPhase("on"); return; }
    setPhase((p) => (p === "on" ? "fading" : p));
    const t = window.setTimeout(() => setPhase("off"), 350); // after the 300 ms fade (or at once with reduced motion)
    return () => clearTimeout(t);
  }, [speech]);

  useEffect(() => {
    const el = root.current;
    if (!el || !speech) return;
    const bars = Array.from(el.children) as HTMLElement[];
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduced) {
      bars.forEach((b, i) => { b.style.transform = `scaleY(${STATIC[i]})`; });
      return;
    }
    let timer = 0;
    const draw = () => {
      const levels = waveLevels(speech, performance.now(), BARS);
      bars.forEach((b, i) => { b.style.transform = `scaleY(${(MIN + (1 - MIN) * levels[i]).toFixed(2)})`; });
      timer = window.setTimeout(draw, TICK_MS);
    };
    draw();
    return () => clearTimeout(timer);
  }, [speech, phase]); // phase: the bars only exist once the phase leaves "off"

  if (phase === "off") return null;
  return (
    <span ref={root} className={`scene-wave ${phase === "fading" ? "out" : ""}`} aria-hidden="true">
      {STATIC.map((h, i) => <i key={i} style={{ transform: `scaleY(${MIN + (1 - MIN) * (phase === "fading" ? 0 : h * 0.3)})` }} />)}
    </span>
  );
}
