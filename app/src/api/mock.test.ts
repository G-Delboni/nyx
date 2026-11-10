import { afterEach, describe, expect, it } from "vitest";
import { EVT_EVENT } from "./contract";
import type { NyxEvent } from "./contract";
import { createMockBackend } from "./mock";

let mock = createMockBackend({ intervalMs: 0 });
afterEach(() => mock.stop());
const fresh = () => (mock = createMockBackend({ intervalMs: 0 }));

describe("mock backend", () => {
  it("snapshot has varied sessions and week near 70%", async () => {
    const s = await fresh().invoke("get_snapshot", {});
    expect(s.agents.length).toBeGreaterThanOrEqual(3);
    expect(new Set(s.agents.map((a) => a.status)).size).toBeGreaterThanOrEqual(3);
    expect(s.weekUsage.percent).toBeGreaterThan(65);
    expect(s.weekUsage.percent).toBeLessThan(75);
    expect(s.status).toBe("alert"); // one session is waiting
  });

  it("marks events read", async () => {
    const m = fresh();
    const before = (await m.invoke("get_snapshot", {})).unreadEvents;
    const ev = (await m.invoke("get_events", { limit: 10 })).find((e) => !e.read)!;
    await m.invoke("mark_event_read", { id: ev.id });
    expect((await m.invoke("get_snapshot", {})).unreadEvents).toBe(before - 1);
  });

  it("does CRUD on contexts", async () => {
    const m = fresh();
    const c = await m.invoke("save_context", { ctx: { id: "", name: "X", rootPath: "C:\\x", instructions: "", color: null, createdAt: "", allowedTools: ["Read", "Grep", "Glob"], references: [], aiEnabled: false } });
    expect(c.id).not.toBe("");
    expect((await m.invoke("list_contexts", {})).some((x) => x.id === c.id)).toBe(true);
    await m.invoke("delete_context", { id: c.id });
    expect((await m.invoke("list_contexts", {})).some((x) => x.id === c.id)).toBe(false);
  });

  it("emits events on tick and unlisten stops them", async () => {
    const m = fresh();
    const got: NyxEvent[] = [];
    const un = await m.listen<NyxEvent>(EVT_EVENT, (e) => got.push(e));
    m.tick();
    expect(got).toHaveLength(1);
    un();
    m.tick();
    expect(got).toHaveLength(1);
  });

  it("usage only has a ceiling for the week", async () => {
    const m = fresh();
    expect((await m.invoke("get_usage", { range: "today" })).percent).toBeNull();
    expect((await m.invoke("get_usage", { range: "week" })).ceilingUsd).toBe(1150);
  });
});

describe("mock voice commands", () => {
  it("reports status with and without the Azure key", async () => {
    const m = fresh();
    const a = await m.invoke("get_voice_status", {});
    expect(a).toMatchObject({ engineInUse: "windows", azureKeyPresent: false, windowsVoice: "Microsoft Maria", monthChars: 12345 });
    m.setAzureKey(true);
    expect((await m.invoke("get_voice_status", {})).engineInUse).toBe("azure");
  });

  it("test_voice emits speech then speech-end; stop_speech interrupts", async () => {
    const m = fresh();
    const seen: string[] = [];
    await m.listen("nyx://speech", () => seen.push("start"));
    await m.listen<{ interrupted: boolean }>("nyx://speech-end", (e) => seen.push(e.interrupted ? "stopped" : "end"));
    const ev = await m.invoke("test_voice", { text: null, voiceName: "pt-BR-ThalitaNeural" });
    expect(ev.kind).toBe("test");
    expect(ev.text).toContain("Voz configurada");
    expect((await m.invoke("get_voice_status", {})).speaking).toBe(true);
    await m.invoke("stop_speech", {});
    expect(seen).toEqual(["start", "stopped"]);
    expect((await m.invoke("get_voice_status", {})).speaking).toBe(false);
  });

  it("set_voice_muted persists in settings", async () => {
    const m = fresh();
    const s = await m.invoke("set_voice_muted", { muted: true });
    expect(s.voiceMuted).toBe(true);
    expect((await m.invoke("get_settings", {})).voiceMuted).toBe(true);
  });
});

describe("mock opening speech and settings event", () => {
  it("speaks the opening only when allowed", async () => {
    const m = fresh();
    const kinds: string[] = [];
    await m.listen<{ kind: string }>("nyx://speech", (e) => kinds.push(e.kind));
    expect(await m.invoke("speak_opening", { text: "De volta, Dev." })).toBe("De volta, Dev.");
    expect(kinds).toEqual(["opening"]);
    const s = await m.invoke("get_settings", {});
    await m.invoke("save_settings", { settings: { ...s, voiceSpeakOn: { ...s.voiceSpeakOn, opening: false } } });
    expect(await m.invoke("speak_opening", { text: "x" })).toBeNull();
    await m.invoke("save_settings", { settings: { ...s, doNotDisturb: true } });
    expect(await m.invoke("speak_opening", { text: "x" })).toBeNull();
    await m.invoke("save_settings", { settings: { ...s, voiceMuted: true } });
    expect(await m.invoke("speak_opening", { text: "x" })).toBeNull();
    expect(kinds).toHaveLength(1);
  });
  it("emits nyx://settings on save and mute", async () => {
    const m = fresh();
    const got: boolean[] = [];
    await m.listen<{ voiceMuted: boolean }>("nyx://settings", (s) => got.push(s.voiceMuted));
    await m.invoke("set_voice_muted", { muted: true });
    await m.invoke("save_settings", { settings: { ...(await m.invoke("get_settings", {})), voiceMuted: false } });
    expect(got).toEqual([true, false]);
  });
});
