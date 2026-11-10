import { describe, expect, it } from "vitest";
import type { ChatMessage } from "../api/contract";
import { inContext, mergeMessage, pendingActions, replyCost, sendBlock } from "./messages";

const msg = (id: number, over: Partial<ChatMessage> = {}): ChatMessage => ({
  id, contextId: null, role: "nyx", text: "x", actions: [], costUsd: 0.001, createdAt: "2026-11-07T10:00:00Z", ...over,
});

describe("sendBlock", () => {
  const o = { keyPresent: true, sending: false };
  it("allows a normal message", () => expect(sendBlock("oi", o)).toBeNull());
  it("blocks by priority: key, sending, empty, length", () => {
    expect(sendBlock("oi", { ...o, keyPresent: false })).toBe("noKey");
    expect(sendBlock("oi", { ...o, sending: true })).toBe("sending");
    expect(sendBlock("   ", o)).toBe("empty");
    expect(sendBlock("a".repeat(2001), o)).toBe("tooLong");
  });
});

describe("mergeMessage", () => {
  it("inserts in order and replaces by id", () => {
    let l = mergeMessage([], msg(2));
    l = mergeMessage(l, msg(1));
    expect(l.map((m) => m.id)).toEqual([1, 2]);
    l = mergeMessage(l, msg(2, { text: "novo" }));
    expect(l).toHaveLength(2);
    expect(l[1].text).toBe("novo");
  });
  it("matches contexts with null as general", () => {
    expect(inContext(msg(1), null)).toBe(true);
    expect(inContext(msg(1, { contextId: "c" }), null)).toBe(false);
    expect(inContext(msg(1, { contextId: "c" }), "c")).toBe(true);
  });
});

describe("actions and cost", () => {
  it("lists only proposed actions", () => {
    const m = msg(1, {
      actions: [
        { id: "a1", kind: "openPanel", label: "x", args: {}, state: "proposed" },
        { id: "a2", kind: "installHooks", label: "y", args: {}, state: "done" },
      ],
    });
    expect(pendingActions([m, msg(2)]).map((p) => p.action.id)).toEqual(["a1"]);
  });
  it("sums only Nyx replies", () => {
    expect(replyCost([msg(1), msg(2, { role: "user", costUsd: 5 }), msg(3)])).toBe(0.002);
  });
});

import { describeAction } from "./messages";

describe("describeAction", () => {
  const a = (kind: ChatMessage["actions"][number]["kind"], args: Record<string, unknown> = {}) =>
    describeAction({ id: "a", kind, label: "", args, state: "proposed" });
  it("describes each kind", () => {
    expect(a("startDelegation", { title: "X", steps: [1, 2, 3] })).toContain('"X" com 3 passos');
    expect(a("approveStep")).toContain("commit");
    expect(a("installHooks")).toContain("backup");
    expect(a("createContext", { name: "Loja" })).toContain('"Loja"');
    expect(a("adjustStep")).toContain("ajuste");
    expect(a("openPanel")).toContain("aba");
  });
});
