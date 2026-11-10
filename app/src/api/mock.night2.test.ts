import { afterEach, describe, expect, it, vi } from "vitest";
import { availableActions, splitFailure } from "../delegation/status";
import { EVT_CHAT, EVT_DELEGATION } from "./contract";
import type { ChatMessage, Delegation } from "./contract";
import { createMockBackend } from "./mock";

let m = createMockBackend({ intervalMs: 0, delegationStepMs: 0, chatDelayMs: 0 });
afterEach(() => { m.stop(); vi.useRealTimers(); });
const fresh = (o: Parameters<typeof createMockBackend>[0] = {}) =>
  (m = createMockBackend({ intervalMs: 0, delegationStepMs: 0, chatDelayMs: 0, ...o }));

const draft = (contextId: string, steps: string[], useAi = false) => ({ contextId, title: "T", steps, useAi, model: null });

describe("mock delegation", () => {
  it("runs steps to done with commits, approving each checkpoint", async () => {
    const b = fresh();
    const d = await b.invoke("create_delegation", { draft: draft("ctx-demo", ["um", "dois", "tres"]) });
    expect(d.status).toBe("draft");
    expect((await b.invoke("start_delegation", { id: d.id })).status).toBe("running");
    for (let i = 0; i < 3; i++) {
      const cp = b.delegations.finishStep(d.id);
      expect(cp.status).toBe("checkpoint");
      expect(cp.steps[i].report?.lines.length).toBeLessThanOrEqual(5);
      const after = await b.invoke("approve_step", { id: d.id });
      expect(after.steps[i].commit).toMatch(/^[0-9a-f]{7}$/);
      expect(after.status).toBe(i === 2 ? "done" : "running");
    }
    expect((await b.invoke("finish_delegation", { id: d.id, merge: true })).status).toBe("done");
  });

  it("advances by itself with timers", async () => {
    vi.useFakeTimers();
    const b = fresh({ delegationStepMs: 3000 });
    const seen: string[] = [];
    await b.listen<Delegation>(EVT_DELEGATION, (x) => seen.push(`${x.status}:${x.steps[x.currentStep].liveActivity ?? "-"}`));
    const d = await b.invoke("create_delegation", { draft: draft("ctx-demo", ["a", "b"]) });
    await b.invoke("start_delegation", { id: d.id });
    await vi.advanceTimersByTimeAsync(3100);
    expect((await b.invoke("get_delegation", { id: d.id })).status).toBe("checkpoint");
    expect(seen.some((s) => s.startsWith("running:Editando"))).toBe(true);
  });

  it("adjusts the same step (attempts + feedback) and reports the AI summary only with key", async () => {
    const b = fresh({ aiKey: true });
    const d = await b.invoke("create_delegation", { draft: draft("ctx-demo", ["a", "b"], true) });
    await b.invoke("start_delegation", { id: d.id });
    const cp = b.delegations.finishStep(d.id);
    expect(cp.steps[0].aiSummary).not.toBeNull();
    const adj = await b.invoke("adjust_step", { id: d.id, feedback: "mais curto" });
    expect(adj.currentStep).toBe(0);
    expect(adj.steps[0].attempts).toBe(2);
    expect(adj.steps[0].feedback).toEqual(["mais curto"]);
    expect(adj.status).toBe("running");

    const b2 = fresh();
    const d2 = await b2.invoke("create_delegation", { draft: draft("ctx-demo", ["a"], true) });
    await b2.invoke("start_delegation", { id: d2.id });
    expect(b2.delegations.finishStep(d2.id).steps[0].aiSummary).toBeNull();
  });

  it("hands off when context fill reaches 60%", async () => {
    const b = fresh();
    const d = await b.invoke("create_delegation", { draft: draft("ctx-demo", ["[passagem] grande", "seguinte"]) });
    await b.invoke("start_delegation", { id: d.id });
    expect(b.delegations.finishStep(d.id).steps[0].contextFill).toBeGreaterThanOrEqual(0.6);
    expect((await b.invoke("approve_step", { id: d.id })).status).toBe("handoff");
    const next = b.delegations.completeHandoff(d.id);
    expect(next.status).toBe("running");
    expect(next.handoffs).toBe(1);
    expect(next.currentStep).toBe(1);
  });

  it("blocks on a tool outside the list, and passes once it is allowed", async () => {
    const b = fresh();
    const d = await b.invoke("create_delegation", { draft: draft("ctx-demo", ["[bloqueio] publicar"]) });
    await b.invoke("start_delegation", { id: d.id });
    const blocked = b.delegations.finishStep(d.id);
    expect(blocked.status).toBe("blocked");
    expect(blocked.blockedReason).toContain("Bash(git push:*)");
    const ctx = (await b.invoke("list_contexts", {})).find((c) => c.id === "ctx-demo")!;
    await b.invoke("save_context", { ctx: { ...ctx, allowedTools: [...ctx.allowedTools, "Bash(git push:*)"] } });
    const again = await b.invoke("adjust_step", { id: d.id, feedback: "liberei a ferramenta" });
    expect(again.status).toBe("running");
    expect(b.delegations.finishStep(d.id).status).toBe("checkpoint");
  });

  it("keeps one active delegation per context and supports cancel", async () => {
    const b = fresh();
    // ctx-nyx already has the seeded checkpoint delegation
    const d = await b.invoke("create_delegation", { draft: draft("ctx-nyx", ["a"]) });
    await expect(b.invoke("start_delegation", { id: d.id })).rejects.toThrow(/ativa/);
    const active = (await b.invoke("list_delegations", { contextId: "ctx-nyx" })).find((x) => x.status === "checkpoint")!;
    expect((await b.invoke("cancel_delegation", { id: active.id })).status).toBe("cancelled");
    expect((await b.invoke("start_delegation", { id: d.id })).status).toBe("running");
    await expect(b.invoke("finish_delegation", { id: active.id, merge: true })).rejects.toThrow();
    expect((await b.invoke("get_snapshot", {})).activeDelegations).toBe(1);
  });

  it("hides finished delegations unless asked and plans only with a key", async () => {
    const b = fresh();
    expect((await b.invoke("list_delegations", { contextId: "ctx-demo" })).length).toBe(0);
    expect((await b.invoke("list_delegations", { contextId: "ctx-demo", includeFinished: true })).length).toBe(1);
    await expect(b.invoke("plan_delegation", { contextId: "ctx-nyx", goal: "x" })).rejects.toThrow(/ANTHROPIC_API_KEY/);
    b.setAiKey(true);
    expect((await b.invoke("plan_delegation", { contextId: "ctx-nyx", goal: "x" })).length).toBeGreaterThan(1);
  });
});

describe("mock chat and AI status", () => {
  it("reports the key toggle", async () => {
    const b = fresh();
    expect((await b.invoke("get_ai_status", {})).keyPresent).toBe(false);
    b.setAiKey(true);
    expect((await b.invoke("get_ai_status", {})).keyPresent).toBe(true);
  });

  it("refuses to chat without a key", async () => {
    await expect(fresh().invoke("send_chat", { contextId: null, text: "oi" })).rejects.toThrow(/ANTHROPIC_API_KEY/);
  });

  it("replies with one proposed action and runs it only on confirmation", async () => {
    const b = fresh({ aiKey: true });
    const got: ChatMessage[] = [];
    await b.listen<ChatMessage>(EVT_CHAT, (x) => got.push(x));
    const reply = await b.invoke("send_chat", { contextId: "ctx-demo", text: "quero delegar uma tarefa" });
    expect(reply.role).toBe("nyx");
    expect(reply.actions).toHaveLength(1);
    expect(reply.actions[0].state).toBe("proposed");
    expect((await b.invoke("list_delegations", { contextId: "ctx-demo" })).length).toBe(0);
    expect((await b.invoke("get_chat", { contextId: "ctx-demo", limit: 10 })).map((x) => x.role)).toEqual(["user", "nyx"]);
    expect((await b.invoke("get_chat", { contextId: null, limit: 10 })).length).toBe(0);
    const done = await b.invoke("run_chat_action", { messageId: reply.id, actionId: reply.actions[0].id });
    expect(done.actions[0].state).toBe("done");
    expect((await b.invoke("list_delegations", { contextId: "ctx-demo" })).some((d) => d.status === "running")).toBe(true);
    await expect(b.invoke("run_chat_action", { messageId: reply.id, actionId: reply.actions[0].id })).rejects.toThrow();
    expect(got.length).toBeGreaterThanOrEqual(3);
    expect((await b.invoke("get_ai_status", {})).weekCostUsd).toBeGreaterThan(0.42);
  });

  it("marks an action failed when the command throws, and clears per context", async () => {
    const b = fresh({ aiKey: true });
    const r = await b.invoke("send_chat", { contextId: "ctx-nyx", text: "aprova o passo" });
    // ctx-nyx has a seeded checkpoint, so cancel it to make approve fail
    const cp = (await b.invoke("list_delegations", { contextId: "ctx-nyx" }))[0];
    await b.invoke("cancel_delegation", { id: cp.id });
    expect((await b.invoke("run_chat_action", { messageId: r.id, actionId: r.actions[0].id })).actions[0].state).toBe("failed");
    await b.invoke("clear_chat", { contextId: "ctx-nyx" });
    expect(await b.invoke("get_chat", { contextId: "ctx-nyx", limit: 10 })).toEqual([]);
  });
});

describe("mock approve in blocked and git failures", () => {
  it("approves a blocked step anyway (commit + next step)", async () => {
    const b = fresh();
    const d = await b.invoke("create_delegation", { draft: draft("ctx-demo", ["[bloqueio] publicar", "depois"]) });
    await b.invoke("start_delegation", { id: d.id });
    expect(b.delegations.finishStep(d.id).status).toBe("blocked");
    const next = await b.invoke("approve_step", { id: d.id });
    expect(next.steps[0].status).toBe("approved");
    expect(next.steps[0].commit).toMatch(/^[0-9a-f]{7}$/);
    expect(next.blockedReason).toBeNull();
    expect(next.status).toBe("running");
    expect(next.currentStep).toBe(1);
  });

  it.each([
    ["[sem-commits]", /não tem commits/, /unknown revision/],
    ["[sem-identidade]", /não sabe quem você é/, /Author identity unknown/],
    ["[pre-commit]", /pre-commit falhou/, /eslint/],
  ])("%s fails the approve once with summary + raw output, then the retry works", async (marker, summary, raw) => {
    const b = fresh();
    const d = await b.invoke("create_delegation", { draft: draft("ctx-demo", [`${marker} fazer`]) });
    await b.invoke("start_delegation", { id: d.id });
    b.delegations.finishStep(d.id);
    const failed = await b.invoke("approve_step", { id: d.id });
    expect(failed.status).toBe("failed");
    expect(failed.steps[0].status).toBe("checkpoint");
    expect(failed.steps[0].commit).toBeNull();
    const f = splitFailure(failed.blockedReason);
    expect(f.summary).toMatch(summary);
    expect(f.output).toMatch(raw);
    expect(availableActions(failed).retryApprove).toBe(true);
    const ok = await b.invoke("approve_step", { id: d.id });
    expect(ok.status).toBe("done");
    expect(ok.steps[0].commit).not.toBeNull();
  });

  it("still rejects approve while running", async () => {
    const b = fresh();
    const d = await b.invoke("create_delegation", { draft: draft("ctx-demo", ["a"]) });
    await b.invoke("start_delegation", { id: d.id });
    await expect(b.invoke("approve_step", { id: d.id })).rejects.toThrow();
  });
});
