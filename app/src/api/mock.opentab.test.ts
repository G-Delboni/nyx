import { afterEach, describe, expect, it } from "vitest";
import { EVT_OPEN_TAB } from "./contract";
import { createMockBackend } from "./mock";

let m = createMockBackend({ intervalMs: 0, delegationStepMs: 0, chatDelayMs: 0, aiKey: true });
afterEach(() => m.stop());

describe("open-tab event", () => {
  it("openTab emits nyx://open-tab with the tab", async () => {
    m = createMockBackend({ intervalMs: 0 });
    const got: Array<{ tab: string }> = [];
    const un = await m.listen<{ tab: string }>(EVT_OPEN_TAB, (p) => got.push(p));
    m.openTab("delegate");
    un();
    m.openTab("chat");
    expect(got).toEqual([{ tab: "delegate" }]);
  });

  it("confirming the chat open-panel action emits the event, and only then", async () => {
    m = createMockBackend({ intervalMs: 0, chatDelayMs: 0, aiKey: true });
    const got: Array<{ tab: string }> = [];
    await m.listen<{ tab: string }>(EVT_OPEN_TAB, (p) => got.push(p));
    const reply = await m.invoke("send_chat", { contextId: null, text: "como está o uso da semana?" });
    expect(reply.actions[0].kind).toBe("openPanel");
    expect(got).toEqual([]);
    const done = await m.invoke("run_chat_action", { messageId: reply.id, actionId: reply.actions[0].id });
    expect(done.actions[0].state).toBe("done");
    expect(got).toEqual([{ tab: "usage" }]);
  });
});
