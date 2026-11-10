import { describe, expect, it } from "vitest";
import type { Delegation } from "../api/contract";
import { addStep, applyPlan, emptyDraft, MAX_STEPS, moveStep, normalizeDraft, removeStep, updateStep, validateDraft } from "./draft";
import {
  approvedCount, availableActions, contextPercent, isActive, progressPercent, stepLabel, upsertDelegation,
} from "./status";
import { applyPreset, detectPreset, formatTools, invalidTools, isBroadBash, parseTools, TOOL_PRESETS } from "./tools";

describe("tool presets", () => {
  it("defines the three presets exactly", () => {
    expect(applyPreset("readonly")).toEqual(["Read", "Grep", "Glob"]);
    expect(applyPreset("edit")).toEqual(["Read", "Grep", "Glob", "Edit", "Write"]);
    expect(applyPreset("edit-tests")).toEqual([
      "Read", "Grep", "Glob", "Edit", "Write", "Bash(npm test:*)", "Bash(npm run build:*)",
    ]);
  });
  it("detects a preset regardless of order, and null for custom lists", () => {
    expect(detectPreset(["Glob", "Read", "Grep"])?.id).toBe("readonly");
    expect(detectPreset(["Read"])).toBeNull();
    expect(TOOL_PRESETS.map((p) => detectPreset(p.tools)?.id)).toEqual(["readonly", "edit", "edit-tests"]);
  });
  it("returns copies, not the shared arrays", () => {
    const a = applyPreset("readonly");
    a.push("Bash");
    expect(applyPreset("readonly")).toHaveLength(3);
  });
  it("parses text by lines or commas, keeping commas inside parentheses", () => {
    expect(parseTools("Read, Grep\nBash(git log:*, --oneline)\n Read ")).toEqual(["Read", "Grep", "Bash(git log:*, --oneline)"]);
    expect(parseTools("")).toEqual([]);
    expect(parseTools(formatTools(["Read", "Edit"]))).toEqual(["Read", "Edit"]);
  });
  it("flags malformed and broad entries", () => {
    expect(invalidTools(["Read", "Bash(npm test:*)", "rm -rf", "Bash(", "(x)"])).toEqual(["rm -rf", "Bash(", "(x)"]);
    expect(isBroadBash("Bash")).toBe(true);
    expect(isBroadBash("Bash(npm test:*)")).toBe(false);
  });
});

describe("draft validation", () => {
  const ctx = { allowedTools: ["Read"] };
  const ok = { ...emptyDraft("c1"), title: "Titulo", steps: ["Fazer algo"] };
  it("accepts a good draft", () => {
    expect(validateDraft(ok, ctx)).toEqual([]);
  });
  it("requires context, title and a step", () => {
    const e = validateDraft(emptyDraft(""), null).map((x) => x.field);
    expect(e).toEqual(expect.arrayContaining(["context", "title", "steps"]));
  });
  it("blocks contexts without allowed tools", () => {
    expect(validateDraft(ok, { allowedTools: [] }).map((x) => x.field)).toContain("tools");
  });
  it("blocks AI without the key and long inputs", () => {
    expect(validateDraft({ ...ok, useAi: true }, ctx, { keyPresent: false })).toHaveLength(1);
    expect(validateDraft({ ...ok, useAi: true }, ctx, { keyPresent: true })).toEqual([]);
    expect(validateDraft({ ...ok, title: "x".repeat(81) }, ctx)).toHaveLength(1);
    const long = validateDraft({ ...ok, steps: ["a", "y".repeat(4001)] }, ctx);
    expect(long[0].step).toBe(1);
  });
  it("normalizes by trimming and dropping empty steps", () => {
    expect(normalizeDraft({ ...ok, title: "  T ", steps: [" a ", "", "  ", "b"] })).toMatchObject({ title: "T", steps: ["a", "b"] });
  });
});

describe("step list editing", () => {
  it("reorders", () => {
    expect(moveStep(["a", "b", "c"], 0, 2)).toEqual(["b", "c", "a"]);
    expect(moveStep(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
    const same = ["a", "b"];
    expect(moveStep(same, 1, 5)).toBe(same);
    expect(moveStep(same, 1, 1)).toBe(same);
  });
  it("adds, updates and removes without mutating", () => {
    const s = ["a", "b"];
    expect(addStep(s, 1, "x")).toEqual(["a", "x", "b"]);
    expect(addStep(s)).toEqual(["a", "b", ""]);
    expect(updateStep(s, 0, "z")).toEqual(["z", "b"]);
    expect(removeStep(s, 0)).toEqual(["b"]);
    expect(removeStep(["only"], 0)).toEqual(["only"]);
    expect(s).toEqual(["a", "b"]);
  });
  it("caps the number of steps", () => {
    const full = Array.from({ length: MAX_STEPS }, (_, i) => String(i));
    expect(addStep(full)).toBe(full);
    expect(applyPlan(["mine", ""], ["p1", "p2"], true)).toEqual(["mine", "p1", "p2"]);
    expect(applyPlan(["mine"], ["p1"], false)).toEqual(["p1"]);
    expect(applyPlan(full, ["p"], true)).toHaveLength(MAX_STEPS);
  });
});

function deleg(over: Partial<Delegation> = {}): Delegation {
  const step = (i: number, status: Delegation["steps"][number]["status"], fill: number | null = null) => ({
    index: i, prompt: "p", status, attempts: 1, report: null, aiSummary: null, feedback: [], commit: null,
    costUsd: 0, contextFill: fill, liveActivity: null, startedAt: null, finishedAt: null,
  });
  return {
    id: "d1", contextId: "c", title: "t", status: "checkpoint", currentStep: 1,
    steps: [step(0, "approved"), step(1, "checkpoint", 0.42), step(2, "pending"), step(3, "pending")],
    worktreePath: "w", branch: "b", baseBranch: "main", claudeSessionId: null, handoffs: 0, blockedReason: null,
    costUsd: 0, createdAt: "2026-11-07T10:00:00Z", updatedAt: "2026-11-07T10:00:00Z", ...over,
  };
}

describe("delegation status helpers", () => {
  it("derives labels and progress", () => {
    const d = deleg();
    expect(stepLabel(d)).toBe("Etapa 2 de 4");
    expect(approvedCount(d)).toBe(1);
    expect(progressPercent(d)).toBe(25);
    expect(contextPercent(d)).toBe(42);
    expect(contextPercent(deleg({ currentStep: 2 }))).toBeNull();
  });
  it("offers the right actions per status", () => {
    expect(availableActions({ status: "draft" })).toMatchObject({ start: true, approve: false, cancel: false });
    expect(availableActions({ status: "checkpoint" })).toMatchObject({ approve: true, adjust: true, cancel: true, finish: false });
    expect(availableActions({ status: "blocked" })).toMatchObject({ approve: false, adjust: true, cancel: true });
    expect(availableActions({ status: "running" })).toMatchObject({ approve: false, adjust: false, cancel: true });
    expect(availableActions({ status: "done" })).toMatchObject({ finish: true, merge: true, cancel: false });
    expect(availableActions({ status: "cancelled" })).toMatchObject({ finish: true, merge: false });
    expect(isActive("handoff")).toBe(true);
    expect(isActive("done")).toBe(false);
  });
  it("upserts newest first", () => {
    const a = deleg({ id: "a", updatedAt: "2026-11-07T10:00:00Z" });
    const b = deleg({ id: "b", updatedAt: "2026-11-07T11:00:00Z" });
    expect(upsertDelegation([a], b).map((x) => x.id)).toEqual(["b", "a"]);
    const a2 = { ...a, updatedAt: "2026-11-07T12:00:00Z", title: "novo" };
    const out = upsertDelegation([b, a], a2);
    expect(out.map((x) => x.id)).toEqual(["a", "b"]);
    expect(out[0].title).toBe("novo");
  });
});

import { extractBlockedTool, formatLines, parseLines, withTool } from "./tools";
import { stepTitle } from "./status";

describe("lines, blocked tool and step title", () => {
  it("parses lines", () => {
    expect(parseLines(" CLAUDE.md \n\ndocs/a.md\r\nCLAUDE.md")).toEqual(["CLAUDE.md", "docs/a.md"]);
    expect(formatLines(["a", "b"])).toBe("a\nb");
  });
  it("extracts the blocked tool", () => {
    expect(extractBlockedTool("O passo precisa de Bash(git push:*), que não está na lista.")).toBe("Bash(git push:*)");
    expect(extractBlockedTool("Permissão negada para WebFetch")).toBe("WebFetch");
    expect(extractBlockedTool("algo estranho")).toBeNull();
    expect(extractBlockedTool(null)).toBeNull();
  });
  it("adds a tool once", () => {
    const t = ["Read"];
    expect(withTool(t, "Edit")).toEqual(["Read", "Edit"]);
    expect(withTool(t, "Read")).toBe(t);
    expect(withTool(t, " ")).toBe(t);
  });
  it("titles a step from its first line", () => {
    expect(stepTitle("\n  Fazer X\nmais detalhes")).toBe("Fazer X");
    expect(stepTitle("a".repeat(100), 10)).toBe("aaaaaaaaa…");
    expect(stepTitle("   ")).toBe("(passo vazio)");
  });
});

import { splitFailure } from "./status";

describe("blocked / failed helpers", () => {
  it("offers approve-anyway only when blocked, retry only for a failed commit", () => {
    expect(availableActions({ status: "blocked" })).toMatchObject({ approveAnyway: true, retryApprove: false });
    expect(availableActions({ status: "checkpoint" }).approveAnyway).toBe(false);
    const d = deleg({ status: "failed", currentStep: 1 }); // step 1 is "checkpoint"
    expect(availableActions(d).retryApprove).toBe(true);
    const e = deleg({ status: "failed", currentStep: 2 }); // step 2 is "pending": agent failure, not a git one
    expect(availableActions(e).retryApprove).toBe(false);
  });
  it("splits failure text into summary and mono output", () => {
    expect(splitFailure("Resumo.\nlinha 1\nlinha 2")).toEqual({ summary: "Resumo.", output: "linha 1\nlinha 2" });
    expect(splitFailure("só resumo")).toEqual({ summary: "só resumo", output: null });
    expect(splitFailure(null)).toEqual({ summary: "O passo falhou.", output: null });
  });
});
