import { describe, expect, it } from "vitest";
import { formatPercent, formatTokens, formatUsd, relativeTime, segments, shortModel, totalTokens } from "./format";

describe("formatTokens", () => {
  it("scales units", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(950)).toBe("950");
    expect(formatTokens(12_345)).toBe("12,3k");
    expect(formatTokens(4_200_000)).toBe("4,2M");
    expect(formatTokens(-5)).toBe("0");
  });
  it("sums all token kinds", () => {
    expect(totalTokens({ input: 1, output: 2, cacheWrite: 3, cacheRead: 4 })).toBe(10);
  });
});

describe("formatUsd", () => {
  it("uses pt-BR separators", () => {
    expect(formatUsd(12.5).replace(/\s/g, " ")).toBe("US$ 12,50");
    expect(formatUsd(1234.567).replace(/\s/g, " ")).toBe("US$ 1.234,57");
  });
});

describe("formatPercent", () => {
  it("handles null", () => {
    expect(formatPercent(null)).toBe("--");
    expect(formatPercent(69.6)).toBe("70%");
  });
});

describe("relativeTime", () => {
  const now = Date.parse("2026-11-07T12:00:00Z");
  const ago = (s: number) => new Date(now - s * 1000).toISOString();
  it("buckets", () => {
    expect(relativeTime(ago(10), now)).toBe("agora");
    expect(relativeTime(ago(5 * 60), now)).toBe("há 5 min");
    expect(relativeTime(ago(2 * 3600), now)).toBe("há 2 h");
    expect(relativeTime(ago(3 * 86400), now)).toBe("há 3 d");
  });
  it("tolerates future and invalid input", () => {
    expect(relativeTime(ago(-100), now)).toBe("agora");
    expect(relativeTime("nope", now)).toBe("--");
  });
});

describe("shortModel", () => {
  it("shortens known ids", () => {
    expect(shortModel("claude-opus-4-5-20251101")).toBe("Opus 4.5");
    expect(shortModel("claude-sonnet-4-5")).toBe("Sonnet 4.5");
    expect(shortModel("claude-haiku-4-5-20251001")).toBe("Haiku 4.5");
    expect(shortModel("gpt-x")).toBe("gpt-x");
  });
});

describe("segments", () => {
  it("clamps", () => {
    expect(segments(0)).toBe(0);
    expect(segments(70)).toBe(14);
    expect(segments(250)).toBe(20);
    expect(segments(null)).toBe(0);
  });
});

import { formatUsdFine } from "./format";
describe("formatUsdFine", () => {
  it("shows sub-cent values with 4 decimals", () => {
    expect(formatUsdFine(0.0008).replace(/\s/g, " ")).toBe("US$ 0,0008");
    expect(formatUsdFine(0).replace(/\s/g, " ")).toBe("US$ 0,00");
    expect(formatUsdFine(1.5).replace(/\s/g, " ")).toBe("US$ 1,50");
  });
});
