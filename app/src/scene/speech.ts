// Pure helpers for the speaking indicator in the scene (portrait and 3D share them).
import type { CurrentSpeech } from "../hooks/useSpeech";
import { envelopeAt } from "../hooks/useSpeech";

/** The NyxLine is one remark, not a transcript: a chat answer is cut at a sentence end (or a word) and marked with "…". */
export function speechCaption(text: string, max = 110): string {
  const t = text.trim().replace(/\s+/g, " ");
  if (t.length <= max) return t;
  const head = t.slice(0, max);
  const sentence = Math.max(head.lastIndexOf(". "), head.lastIndexOf("? "), head.lastIndexOf("! "));
  if (sentence >= max * 0.4) return head.slice(0, sentence + 1);
  const space = head.lastIndexOf(" ");
  return `${head.slice(0, space > max * 0.5 ? space : max).replace(/[,;:]$/, "")}…`;
}

/** Typing step (ms per glyph) so the caption lands a little before the audio ends; never faster than the HUD's 32 ms. */
export function typingStepFor(speech: CurrentSpeech, captionLength: number, baseMs = 32, maxMs = 70): number {
  if (captionLength <= 0) return baseMs;
  const perChar = (speech.speech.durationMs * 0.85) / captionLength;
  return Math.round(Math.min(maxMs, Math.max(baseMs, perChar)));
}

/**
 * Bar heights (0..1) for the wave glyph at time `now`: the middle bar is the current level, the outer bars trail it
 * (one and two envelope steps back) and are scaled down, so the glyph reads as a small waveform, not a VU meter.
 */
export function waveLevels(speech: CurrentSpeech | null, now: number, bars = 5): number[] {
  const out: number[] = [];
  const mid = (bars - 1) / 2;
  for (let i = 0; i < bars; i++) {
    const d = Math.abs(i - mid);
    const level = envelopeAt(speech, now - d * 50);
    out.push(Math.min(1, Math.max(0, level * (1 - d * 0.22))));
  }
  return out;
}
