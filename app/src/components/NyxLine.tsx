import { useEffect, useRef, type ReactNode } from "react";

// Typing animation, spec from prototypes/panel-hud/e.html:
// - full text in aria-label (role=status); the visible glyphs are aria-hidden spans drawn char by char;
// - 1 new char every 32 ms; 260 ms pause after "." (end of sentence);
// - each new char is born as a random glyph and flicks 2-3 times (40 ms each) before settling; spaces settle at once;
// - glyphs: half-width katakana + #%&@▮; the cursor ▮ follows the text, blinks (1 Hz, steps) only when done;
// - plays on mount and again only when the text changes; with reduced motion the text appears ready.
const GLYPHS = "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉ#%&@▮";
const STEP = 32;
const FLICK = 40;
const PAUSE = 260;

export interface NyxLineProps {
  text: string;
  className?: string;
  /** Ms per glyph (default 32). While she speaks, the caption is paced to the audio (see scene/speech.ts). */
  stepMs?: number;
  /** Show this text ready, without typing (used when the line comes back after a speech). */
  instant?: boolean;
  /** Inline glyph before the text (the speaking wave). */
  lead?: ReactNode;
}

/** Nyx's one-line remark with the cyberpunk typing. Pure presentation: pass the text from `nyxLine()`. */
export function NyxLine({ text, className = "", stepMs = STEP, instant = false, lead }: NyxLineProps) {
  const out = useRef<HTMLSpanElement>(null);
  const root = useRef<HTMLParagraphElement>(null);
  const opts = useRef({ stepMs, instant });
  opts.current = { stepMs, instant };

  useEffect(() => {
    const el = out.current;
    const p = root.current;
    if (!el || !p) return;
    const timers: number[] = [];
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const { stepMs: step, instant: ready } = opts.current;
    el.textContent = "";
    // Hidden window, reduced motion or a text that should just be there: no animation.
    if (reduced || document.hidden || ready) {
      el.textContent = text;
      p.classList.remove("typing");
      return;
    }
    p.classList.add("typing");
    let i = 0;
    const next = () => {
      if (i >= text.length) { p.classList.remove("typing"); return; }
      const ch = text[i++];
      const s = document.createElement("span");
      el.appendChild(s);
      if (ch === " ") s.textContent = " ";
      else {
        let n = 2 + Math.floor(Math.random() * 2);
        const flick = () => {
          if (n-- > 0) {
            s.textContent = GLYPHS[Math.floor(Math.random() * GLYPHS.length)];
            s.className = "x";
            timers.push(window.setTimeout(flick, FLICK));
          } else {
            s.textContent = ch;
            s.className = "";
          }
        };
        flick();
      }
      timers.push(window.setTimeout(next, ch === "." && i < text.length ? PAUSE : step));
    };
    next();
    return () => { timers.forEach(clearTimeout); };
  }, [text]);

  return (
    <p ref={root} className={`scene-say ${className}`} role="status" aria-label={text}>
      {lead}
      <span ref={out} className="t" aria-hidden="true" />
      <span className="cursor" aria-hidden="true" />
    </p>
  );
}
