import { describe, expect, it } from "vitest";
import type { CurrentSpeech } from "../hooks/useSpeech";
import { speechCaption, typingStepFor, waveLevels } from "./speech";

function cur(envelope: number[], durationMs = envelope.length * 50, text = "Online, Dev."): CurrentSpeech {
  return { startedAt: 1000, speech: { id: "s1", kind: "test", text, engine: "windows", durationMs, envelope } };
}

describe("speechCaption", () => {
  it("keeps a short line as is (whitespace collapsed)", () => {
    expect(speechCaption("  Online,   Dev. ")).toBe("Online, Dev.");
  });
  it("cuts a long answer at the last sentence end inside the limit", () => {
    const t = "Primeira frase curta. Segunda frase que explica mais um pouco. Terceira frase que não cabe na linha da cena de jeito nenhum.";
    expect(speechCaption(t, 70)).toBe("Primeira frase curta. Segunda frase que explica mais um pouco.");
  });
  it("falls back to a word boundary with an ellipsis when there is no sentence end", () => {
    const t = "uma resposta sem pontuação que segue falando e falando sobre o estado dos agentes e das delegações da noite";
    const c = speechCaption(t, 60);
    expect(c.endsWith("…")).toBe(true);
    expect(c.length).toBeLessThanOrEqual(61);
    expect(c).not.toMatch(/\s…$/);
  });
});

describe("typingStepFor", () => {
  it("never types faster than the HUD's 32 ms", () => {
    expect(typingStepFor(cur([1], 300), 40)).toBe(32);
  });
  it("slows down to land a little before the audio ends, capped at 70 ms", () => {
    expect(typingStepFor(cur([1], 4000), 60)).toBe(57); // 4000*0.85/60
    expect(typingStepFor(cur([1], 40000), 60)).toBe(70);
  });
});

describe("waveLevels", () => {
  it("is flat at silence", () => {
    expect(waveLevels(null, 5000)).toEqual([0, 0, 0, 0, 0]);
  });
  it("centre bar follows the current sample, outer bars trail and shrink", () => {
    const s = cur([0.2, 0.4, 1.0]);
    const l = waveLevels(s, 1000 + 2 * 50 + 1); // on the third sample
    expect(l[2]).toBeCloseTo(1.0, 5);
    expect(l[1]).toBeCloseTo(0.4 * 0.78, 5);
    expect(l[0]).toBeCloseTo(0.2 * 0.56, 5);
    expect(l[3]).toBe(l[1]);
  });
});
