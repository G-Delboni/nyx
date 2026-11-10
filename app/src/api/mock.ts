// In-browser mock backend with realistic data. Used when there is no Tauri runtime.
import type {
  AgentSession, ChatAction, Commands, HooksReport, HooksStatus, ModelUsage, NyxEvent, ProjectContext, Settings, Snapshot, SpeechEvent,
  TokenCounts, VoiceStatus, UsageRange, UsageSummary,
} from "./contract";
import { EVT_AGENTS, EVT_EVENT, EVT_OPEN_TAB, EVT_SETTINGS, EVT_SPEECH, EVT_SPEECH_END, EVT_USAGE } from "./contract";
import { createChatMock } from "./mock-chat";
import { createDelegationMock } from "./mock-delegation";
import type { Backend, OpenTab, Unlisten } from "./client";

const MIN = 60_000;
const HOUR = 60 * MIN;

export interface MockBackend extends Backend {
  /** Advance the simulation one step (emits events). Exposed for tests. */
  tick(): void;
  stop(): void;
  /** Dev toggle: pretend ANTHROPIC_API_KEY is (not) present. */
  setAiKey(present: boolean): void;
  /** Dev/test: simulate the core asking the panel to open a tab (nyx://open-tab). */
  openTab(tab: OpenTab): void;
  /** Dev: fake a spoken line (SpeechEvent with a synthetic mouth envelope), then speech-end. */
  speak(text: string, kind?: import("./contract").SpeechKind): import("./contract").SpeechEvent;
  /** Dev toggle: pretend NYX_AZURE_SPEECH_KEY/REGION are (not) set. Affects get_voice_status and the engine of test_voice. */
  setAzureKey(present: boolean): void;
  /** Direct access to the delegation simulation (tests drive it step by step). */
  delegations: ReturnType<typeof createDelegationMock>;
}

export interface MockOptions {
  now?: () => number;
  /** Interval between simulated events; 0 disables the timer. Never below 10 s. */
  intervalMs?: number;
  /** Duration of one simulated delegation step (default 6 s; 0 = manual, tests call finishStep). */
  delegationStepMs?: number;
  /** Simulated Haiku latency for chat replies (default 700 ms). */
  chatDelayMs?: number;
  /** Initial value of the dev API-key toggle (default false). */
  aiKey?: boolean;
}

const tok = (input: number, output: number, cacheWrite: number, cacheRead: number): TokenCounts =>
  ({ input, output, cacheWrite, cacheRead });

const addTok = (a: TokenCounts, b: TokenCounts): TokenCounts =>
  tok(a.input + b.input, a.output + b.output, a.cacheWrite + b.cacheWrite, a.cacheRead + b.cacheRead);

const scaleTok = (t: TokenCounts, k: number): TokenCounts =>
  tok(Math.round(t.input * k), Math.round(t.output * k), Math.round(t.cacheWrite * k), Math.round(t.cacheRead * k));

export function createMockBackend(opts: MockOptions = {}): MockBackend {
  const now = opts.now ?? Date.now;
  const iso = (offsetMs: number) => new Date(now() + offsetMs).toISOString();

  const contexts: ProjectContext[] = [
    { id: "ctx-demo", name: "Projeto Demo", rootPath: "C:\\Users\\dev\\Documents\\projeto demo",
      instructions: "Responder em pt-BR. Nunca publicar sem aprovação.", color: "#8B5CF6", createdAt: iso(-20 * 24 * HOUR),
      allowedTools: ["Read", "Grep", "Glob"], references: ["docs/notas.md"], aiEnabled: false },
    { id: "ctx-nyx", name: "Nyx", rootPath: "C:\\Users\\dev\\Documents\\nyx",
      instructions: "Mudanças pequenas e revisáveis.", color: "#C084FC", createdAt: iso(-8 * 24 * HOUR),
      allowedTools: ["Read", "Grep", "Glob", "Edit", "Write", "Bash(npm test:*)"], references: ["CLAUDE.md", "docs/PLANO.md"], aiEnabled: true },
  ];

  let settings: Settings = {
    planPreset: "max5x", weeklyCeilingUsd: 1150, alertThresholds: [80, 95],
    doNotDisturb: false, dndSchedule: { start: "22:00", end: "08:00" },
    autostart: false, dataDir: "C:\\Users\\dev\\AppData\\Roaming\\Nyx", longTaskMinutes: 10,
    sceneMode: "portrait",
    asciiMode: "on",
    voiceEnabled: true, voiceEngine: "auto", voiceName: "pt-BR-FranciscaNeural", voiceVolume: 0.8,
    voiceRate: -5, voicePitch: -5, voiceSpeakOn: { alerts: true, opening: true, chat: true, longTask: true },
    voiceMuted: false,
  };

  let agents: AgentSession[] = [
    { sessionId: "s-1a2b", agent: "claude-code", cwd: contexts[1].rootPath, folderName: "nyx", contextId: "ctx-nyx",
      status: "running", startedAt: iso(-48 * MIN), updatedAt: iso(-20_000), lastMessage: null,
      tokens: tok(18_400, 61_200, 210_000, 3_450_000), costUsd: 7.82 },
    { sessionId: "s-3c4d", agent: "claude-code", cwd: contexts[0].rootPath, folderName: "projeto demo", contextId: "ctx-demo",
      status: "waiting", startedAt: iso(-2 * HOUR), updatedAt: iso(-6 * MIN),
      lastMessage: "Claude precisa da sua permissão para rodar o script de render.",
      tokens: tok(9_100, 28_700, 120_500, 1_200_000), costUsd: 3.41 },
    { sessionId: "s-5e6f", agent: "claude-code", cwd: "C:\\Users\\dev\\Documents\\loja-api", folderName: "loja-api", contextId: null,
      status: "done", startedAt: iso(-5 * HOUR), updatedAt: iso(-47 * MIN), lastMessage: null,
      tokens: tok(4_300, 15_900, 60_000, 640_000), costUsd: 1.74 },
    { sessionId: "s-7a8b", agent: "claude-code", cwd: "C:\\Users\\dev\\Documents\\scripts", folderName: "scripts", contextId: null,
      status: "idle", startedAt: iso(-26 * HOUR), updatedAt: iso(-3 * HOUR), lastMessage: null,
      tokens: tok(1_200, 3_800, 12_000, 90_000), costUsd: 0.31 },
  ];

  const NYX_EVENTS = ["Stop", "SubagentStop", "Notification", "PreToolUse", "SessionStart", "SessionEnd", "UserPromptSubmit"];
  let hooksInstalled = false;
  const hooksStatus = (): HooksStatus => ({
    settingsPath: "C:\\Users\\dev\\.claude\\settings.json",
    hookExe: "C:\\Program Files\\Nyx\\nyx-hook.exe",
    hookExeExists: true,
    installed: hooksInstalled,
    missingEvents: hooksInstalled ? [] : [...NYX_EVENTS],
    otherHooks: 1,
  });
  function hooksOp(install: boolean, dryRun: boolean): HooksReport {
    const changed = install !== hooksInstalled;
    if (!dryRun) hooksInstalled = install;
    const status: HooksStatus = dryRun
      ? { ...hooksStatus(), installed: install, missingEvents: install ? [] : [...NYX_EVENTS] }
      : hooksStatus();
    const sign = install ? "+" : "-";
    const diff = changed
      ? NYX_EVENTS.map((e) => `${sign} hooks.${e}: nyx-hook.exe event ${e}`).join("\n") + "\n  1 hook de outro app mantido"
      : "Nada a mudar.";
    return {
      dryRun, changed, diff, status,
      backupPath: !dryRun && changed ? "C:\\Users\\dev\\.claude\\settings.json.nyx-backup" : null,
    };
  }

  let nextEventId = 5;
  let events: NyxEvent[] = [
    { id: 4, kind: "needsInput", priority: "high", sessionId: "s-3c4d", contextId: "ctx-demo",
      title: "Dev, o Claude Code em projeto demo parou.", detail: "Quer uma resposta sua.", createdAt: iso(-6 * MIN), read: false },
    { id: 3, kind: "taskDone", priority: "high", sessionId: "s-5e6f", contextId: null,
      title: "Claude Code concluiu em loja-api.", detail: null, createdAt: iso(-47 * MIN), read: false },
    { id: 2, kind: "usageThreshold", priority: "medium", sessionId: null, contextId: null,
      title: "Semana em 70% do teto.", detail: "Faltam 3 dias.", createdAt: iso(-3 * HOUR), read: true },
    { id: 1, kind: "sessionStart", priority: "low", sessionId: "s-1a2b", contextId: "ctx-nyx",
      title: "Sessão iniciada em nyx.", detail: null, createdAt: iso(-48 * MIN), read: true },
  ];

  const weekModels: ModelUsage[] = [
    { model: "claude-opus-4-5-20251101", tokens: tok(210_000, 640_000, 2_900_000, 41_000_000), costUsd: 562.0, messages: 612 },
    { model: "claude-sonnet-4-5-20250929", tokens: tok(95_000, 310_000, 1_100_000, 18_500_000), costUsd: 229.0, messages: 340 },
    { model: "claude-haiku-4-5-20251001", tokens: tok(40_000, 120_000, 300_000, 2_000_000), costUsd: 14.0, messages: 95 },
  ];

  function usage(range: UsageRange, contextId?: string | null): UsageSummary {
    const scale = range === "week" ? 1 : range === "today" ? 0.22 : 0.07;
    const k = contextId ? scale * 0.4 : scale;
    const byModel: ModelUsage[] = weekModels.map((m) => ({
      model: m.model,
      tokens: scaleTok(m.tokens, k),
      costUsd: +(m.costUsd * k).toFixed(2),
      messages: Math.round(m.messages * k),
    }));
    const tokens = byModel.reduce((acc, m) => addTok(acc, m.tokens), tok(0, 0, 0, 0));
    const costUsd = +byModel.reduce((s, m) => s + m.costUsd, 0).toFixed(2);
    const ceilingUsd = range === "week" ? settings.weeklyCeilingUsd : null;
    const spanMs = range === "week" ? 7 * 24 * HOUR : range === "today" ? 9 * HOUR : 48 * MIN;
    return {
      range, contextId: contextId ?? null, from: iso(-spanMs), to: iso(0), tokens, costUsd,
      sessions: range === "week" ? 23 : range === "today" ? 4 : 1,
      messages: byModel.reduce((s, m) => s + m.messages, 0), byModel,
      ceilingUsd, percent: ceilingUsd ? +((costUsd / ceilingUsd) * 100).toFixed(1) : null,
    };
  }

  function snapshot(): Snapshot {
    const waiting = agents.some((a) => a.status === "waiting");
    const running = agents.some((a) => a.status === "running");
    const week = usage("week");
    return {
      status: waiting || (week.percent ?? 0) >= 95 ? "alert" : running ? "analyzing" : "active",
      activeDelegations: dm.activeCount(), agents, unreadEvents: events.filter((e) => !e.read).length,
      weekUsage: week, settings, contexts,
    };
  }

  let keyPresent = opts.aiKey ?? false;
  let azureKey = false;
  let speaking = false;
  let lastSpeechId: string | null = null;
  let speechTimer: ReturnType<typeof setTimeout> | null = null;
  const MONTH_CHARS = 12_345;

  function voiceStatus(): VoiceStatus {
    const engine = settings.voiceEngine;
    const useAzure = azureKey && engine !== "windows";
    return {
      engineInUse: useAzure ? "azure" : "windows", azureKeyPresent: azureKey, azureRegion: azureKey ? "brazilsouth" : null,
      windowsVoice: "Microsoft Maria", monthChars: MONTH_CHARS, freeTierChars: 500_000, speaking, lastError: null,
    };
  }

  const listeners = new Map<string, Set<(p: never) => void>>();
  const emit = (name: string, payload: unknown) =>
    listeners.get(name)?.forEach((cb) => cb(payload as never));

  const dm = createDelegationMock({
    now, emit, keyPresent: () => keyPresent, stepMs: opts.delegationStepMs ?? 6000,
    allowedTools: (id) => contexts.find((c) => c.id === id)?.allowedTools ?? [],
    contextExists: (id) => contexts.some((c) => c.id === id),
  });

  const cm = createChatMock({
    now, emit, keyPresent: () => keyPresent, replyDelayMs: opts.chatDelayMs ?? 700,
    runAction: async (action: ChatAction, contextId: string | null) => {
      const x = action.args as Record<string, unknown>;
      if (action.kind === "installHooks") { hooksOp(true, false); return; }
      if (action.kind === "startDelegation") {
        const d = dm.create({
          contextId: (x.contextId as string | null) ?? contextId ?? contexts[0].id,
          title: String(x.title ?? "Delegação"), steps: (x.steps as string[]) ?? [], useAi: false, model: null,
        });
        dm.start(d.id);
        return;
      }
      if (action.kind === "approveStep") {
        const d = dm.list(contextId, false).find((y) => y.status === "checkpoint");
        if (!d) throw new Error("Nenhum checkpoint.");
        dm.approve(d.id);
        return;
      }
      if (action.kind === "createContext") {
        contexts.push({ id: `ctx-${contexts.length + 1}`, name: String(x.name ?? "Novo"), rootPath: String(x.rootPath ?? ""),
          instructions: "", color: null, createdAt: iso(0), allowedTools: ["Read", "Grep", "Glob"], references: [], aiEnabled: false });
      }
      if (action.kind === "openPanel") {
        emit(EVT_OPEN_TAB, { tab: String(x.tab ?? "agents") });
        return;
      }
      // adjustStep needs the UI; the mock only acknowledges it.
    },
  });

  function pushEvent(e: Omit<NyxEvent, "id" | "createdAt" | "read">) {
    const ev: NyxEvent = { ...e, id: nextEventId++, createdAt: iso(0), read: false };
    events = [ev, ...events];
    emit(EVT_EVENT, ev);
    emit(EVT_AGENTS, agents);
    emit(EVT_USAGE, usage("week"));
  }

  const rotation: Array<() => void> = [
    () => {
      agents = agents.map((a) => a.sessionId === "s-1a2b"
        ? { ...a, status: "done", updatedAt: iso(0), tokens: addTok(a.tokens, tok(900, 3_100, 8_000, 140_000)), costUsd: +(a.costUsd + 0.42).toFixed(2) }
        : a);
      pushEvent({ kind: "taskDone", priority: "high", sessionId: "s-1a2b", contextId: "ctx-nyx", title: "Claude Code concluiu em nyx.", detail: null });
    },
    () => {
      agents = agents.map((a) => a.sessionId === "s-1a2b" ? { ...a, status: "running", updatedAt: iso(0) } : a);
      pushEvent({ kind: "promptSubmitted", priority: "low", sessionId: "s-1a2b", contextId: "ctx-nyx", title: "Novo prompt em nyx.", detail: null });
    },
    () => {
      agents = agents.map((a) => a.sessionId === "s-3c4d" ? { ...a, status: "running", updatedAt: iso(0), lastMessage: null } : a);
    },
  ];
  let step = 0;

  function tick() {
    rotation[step % rotation.length]();
    step++;
    emit(EVT_AGENTS, agents);
  }

  const interval = opts.intervalMs ?? 12_000;
  const timer = interval > 0 && typeof setInterval !== "undefined"
    ? setInterval(tick, Math.max(10_000, interval)) : null;

  function stopSpeech() {
    if (!speaking || !lastSpeechId) return;
    if (speechTimer) clearTimeout(speechTimer);
    emit(EVT_SPEECH_END, { id: lastSpeechId, interrupted: true });
    speaking = false; lastSpeechId = null; speechTimer = null;
  }

  async function handle<K extends keyof Commands>(cmd: K, args: Commands[K]["args"]): Promise<Commands[K]["result"]> {
    const a = args as Record<string, unknown>;
    let r: unknown = null;
    switch (cmd) {
      case "get_snapshot": r = snapshot(); break;
      case "get_events": {
        const list = a.contextId ? events.filter((e) => e.contextId === a.contextId) : events;
        r = list.slice(0, a.limit as number); break;
      }
      case "get_usage": r = usage(a.range as UsageRange, a.contextId as string | null); break;
      case "list_contexts": r = contexts; break;
      case "save_context": {
        const c = a.ctx as ProjectContext;
        const saved: ProjectContext = c.id
          ? c : { ...c, id: `ctx-${Math.random().toString(36).slice(2, 8)}`, createdAt: iso(0) };
        const i = contexts.findIndex((x) => x.id === saved.id);
        if (i >= 0) contexts[i] = saved; else contexts.push(saved);
        r = saved; break;
      }
      case "delete_context": {
        const i = contexts.findIndex((x) => x.id === a.id);
        if (i >= 0) contexts.splice(i, 1);
        break;
      }
      case "get_settings": r = settings; break;
      case "save_settings": settings = a.settings as Settings; emit(EVT_SETTINGS, settings); r = settings; break;
      case "export_usage_csv": r = `C:\\Users\\dev\\Documents\\Nyx\\usage-${a.range}-${iso(0).slice(0, 10)}.csv`; break;
      case "get_hooks_status": r = hooksStatus(); break;
      case "install_hooks": r = hooksOp(true, a.dryRun as boolean); break;
      case "uninstall_hooks": r = hooksOp(false, a.dryRun as boolean); break;
      case "list_delegations": r = dm.list(a.contextId as string | null, a.includeFinished as boolean); break;
      case "get_delegation": r = dm.get(a.id as string); break;
      case "create_delegation": r = dm.create(a.draft as never); break;
      case "plan_delegation": r = dm.plan(a.contextId as string, a.goal as string); break;
      case "start_delegation": r = dm.start(a.id as string); break;
      case "approve_step": r = dm.approve(a.id as string); break;
      case "adjust_step": r = dm.adjust(a.id as string, a.feedback as string); break;
      case "cancel_delegation": r = dm.cancel(a.id as string); break;
      case "finish_delegation": r = dm.finish(a.id as string, a.merge as boolean); break;
      case "get_chat": r = cm.get((a.contextId as string | null) ?? null, a.limit as number); break;
      case "send_chat": r = await cm.send((a.contextId as string | null) ?? null, a.text as string); break;
      case "clear_chat": cm.clear((a.contextId as string | null) ?? null); break;
      case "run_chat_action": r = await cm.runAction(a.messageId as number, a.actionId as string); break;
      case "get_ai_status":
        r = { keyPresent, model: "claude-haiku-4-5", weekCostUsd: +(0.42 + cm.totalCost()).toFixed(4) }; break;
      case "get_voice_status": r = voiceStatus(); break;
      case "test_voice": {
        const text = (a.text as string | null | undefined) || "De volta, Dev. Voz configurada.";
        r = backend.speak(text, "test"); break;
      }
      case "set_voice_muted":
        settings = { ...settings, voiceMuted: a.muted as boolean };
        emit(EVT_SETTINGS, settings);
        if (a.muted) stopSpeech();
        r = settings; break;
      case "speak_opening": {
        const q = settings;
        const silent = !q.voiceEnabled || q.voiceMuted || !q.voiceSpeakOn.opening || q.doNotDisturb;
        const text = a.text as string;
        if (silent || !text.trim()) { r = null; break; }
        backend.speak(text, "opening");
        r = text; break;
      }
      case "stop_speech": stopSpeech(); break;
      case "mark_event_read":
        events = events.map((e) => e.id === a.id ? { ...e, read: true } : e);
        break;
    }
    return structuredClone(r) as Commands[K]["result"];
  }

  const backend: MockBackend = {
    invoke: handle,
    async listen<T>(event: string, cb: (p: T) => void): Promise<Unlisten> {
      const set = listeners.get(event) ?? new Set();
      set.add(cb as (p: never) => void);
      listeners.set(event, set);
      return () => { set.delete(cb as (p: never) => void); };
    },
    tick,
    stop: () => { if (timer) clearInterval(timer); if (speechTimer) clearTimeout(speechTimer); dm.stop(); },
    setAiKey: (present) => { keyPresent = present; },
    openTab: (tab) => emit(EVT_OPEN_TAB, { tab }),
    setAzureKey: (present) => { azureKey = present; },
    speak: (text, kind = "test") => {
      const durationMs = Math.max(900, text.length * 62);
      const n = Math.ceil(durationMs / 50);
      const envelope = Array.from({ length: n }, (_, i) => +(Math.abs(Math.sin(i * 0.9)) * (0.55 + 0.4 * Math.abs(Math.sin(i * 0.23)))).toFixed(2));
      const id = `mock-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
      stopSpeech();
      const ev: SpeechEvent = { id, kind, text, engine: voiceStatus().engineInUse === "azure" ? "azure" : "windows", durationMs, envelope };
      speaking = true;
      lastSpeechId = id;
      emit(EVT_SPEECH, ev);
      speechTimer = setTimeout(() => {
        speaking = false; lastSpeechId = null; speechTimer = null;
        emit(EVT_SPEECH_END, { id, interrupted: false });
      }, durationMs);
      return ev;
    },
    delegations: dm,
  };
  return backend;
}
