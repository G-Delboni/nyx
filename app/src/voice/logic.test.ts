import { describe, expect, it } from "vitest";
import { clampOffset, clampVolume, formatChars, formatOffset, formatVolume, freeTierPercent, silenceReason, AZURE_VOICES } from "./logic";

describe("voice logic", () => {
  it("clamps offsets to -20..20 integers", () => {
    expect(clampOffset(35)).toBe(20);
    expect(clampOffset(-99)).toBe(-20);
    expect(clampOffset(-4.6)).toBe(-5);
    expect(clampOffset(NaN)).toBe(-20);
  });
  it("clamps volume to 0..1", () => {
    expect(clampVolume(2)).toBe(1);
    expect(clampVolume(-1)).toBe(0);
    expect(clampVolume(0.804)).toBe(0.8);
  });
  it("formats", () => {
    expect(formatOffset(-5)).toBe("−5%");
    expect(formatOffset(0)).toBe("0%");
    expect(formatOffset(10)).toBe("+10%");
    expect(formatVolume(0.8)).toBe("80%");
    expect(formatChars(12345)).toMatch(/^12\.345$/);
  });
  it("free tier percent", () => {
    expect(freeTierPercent(250_000)).toBe(50);
    expect(freeTierPercent(900_000)).toBe(100);
    expect(freeTierPercent(5, 0)).toBe(0);
  });
  it("silence reason", () => {
    const base = { voiceEnabled: true, voiceMuted: false, doNotDisturb: false };
    expect(silenceReason(base)).toBeNull();
    expect(silenceReason({ ...base, doNotDisturb: true })).toMatch(/Não perturbe/);
    expect(silenceReason({ ...base, voiceMuted: true })).toMatch(/Mudo/);
    expect(silenceReason({ ...base, voiceEnabled: false })).toMatch(/desligada/);
  });
  it("lists the 9 voices with Francisca default", () => {
    expect(AZURE_VOICES).toHaveLength(9);
    expect(AZURE_VOICES[0].label).toBe("Francisca (padrão)");
  });
});
