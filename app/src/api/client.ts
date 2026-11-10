// Typed wrapper over Tauri invoke/listen. Falls back to the in-browser mock
// when running outside Tauri (npm run dev in a regular browser).
import type { AgentSession, ChatMessage, Settings, Commands, Delegation, DelegationDraft, NyxEvent, SpeechEvent, UsageSummary } from "./contract";
import { EVT_AGENTS, EVT_CHAT, EVT_DELEGATION, EVT_EVENT, EVT_OPEN_TAB, EVT_SETTINGS, EVT_SPEECH, EVT_SPEECH_END, EVT_USAGE } from "./contract";
import { createMockBackend } from "./mock";

export type Unlisten = () => void;

/** Tabs the core (tray, toast, chat action) can ask the panel to open. */
export type OpenTab = "agents" | "delegate" | "usage" | "events" | "contexts" | "settings" | "chat";

export interface Backend {
  invoke<K extends keyof Commands>(cmd: K, args: Commands[K]["args"]): Promise<Commands[K]["result"]>;
  listen<T>(event: string, cb: (payload: T) => void): Promise<Unlisten>;
}

export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriBackend(): Promise<Backend> {
  const [{ invoke }, { listen }] = await Promise.all([
    import("@tauri-apps/api/core"),
    import("@tauri-apps/api/event"),
  ]);
  return {
    invoke: (cmd, args) => invoke(cmd, args as Record<string, unknown>),
    listen: async (event, cb) => listen(event, (e) => cb(e.payload as never)),
  };
}

function devMock(): Backend {
  const q = typeof location !== "undefined" ? new URLSearchParams(location.search) : null;
  const mock = createMockBackend({ aiKey: q?.get("ai") === "1" });
  // Dev toggle: window.__nyxMock.setAiKey(true) simulates ANTHROPIC_API_KEY being present.
  if (typeof window !== "undefined") (window as unknown as { __nyxMock?: unknown }).__nyxMock = mock;
  return mock;
}

let backendPromise: Promise<Backend> | null = null;
function backend(): Promise<Backend> {
  backendPromise ??= isTauri() ? tauriBackend() : Promise.resolve(devMock());
  return backendPromise;
}

async function call<K extends keyof Commands>(cmd: K, args: Commands[K]["args"]) {
  return (await backend()).invoke(cmd, args);
}

const none = {} as Record<string, never>;

export const api = {
  getSnapshot: () => call("get_snapshot", none),
  getEvents: (limit = 50, contextId?: string | null) => call("get_events", { limit, contextId }),
  getUsage: (range: Commands["get_usage"]["args"]["range"], contextId?: string | null) =>
    call("get_usage", { range, contextId }),
  listContexts: () => call("list_contexts", none),
  saveContext: (ctx: Commands["save_context"]["args"]["ctx"]) => call("save_context", { ctx }),
  deleteContext: (id: string) => call("delete_context", { id }),
  getSettings: () => call("get_settings", none),
  saveSettings: (settings: Commands["save_settings"]["args"]["settings"]) => call("save_settings", { settings }),
  exportUsageCsv: (range: Commands["export_usage_csv"]["args"]["range"], contextId?: string | null) =>
    call("export_usage_csv", { range, contextId }),
  markEventRead: (id: number) => call("mark_event_read", { id }),
  listDelegations: (contextId?: string | null, includeFinished = false) =>
    call("list_delegations", { contextId, includeFinished }),
  getDelegation: (id: string) => call("get_delegation", { id }),
  createDelegation: (draft: DelegationDraft) => call("create_delegation", { draft }),
  planDelegation: (contextId: string, goal: string) => call("plan_delegation", { contextId, goal }),
  startDelegation: (id: string) => call("start_delegation", { id }),
  approveStep: (id: string) => call("approve_step", { id }),
  adjustStep: (id: string, feedback: string) => call("adjust_step", { id, feedback }),
  cancelDelegation: (id: string) => call("cancel_delegation", { id }),
  finishDelegation: (id: string, merge: boolean) => call("finish_delegation", { id, merge }),
  getChat: (contextId: string | null, limit = 50) => call("get_chat", { contextId, limit }),
  sendChat: (contextId: string | null, text: string) => call("send_chat", { contextId, text }),
  clearChat: (contextId: string | null) => call("clear_chat", { contextId }),
  runChatAction: (messageId: number, actionId: string) => call("run_chat_action", { messageId, actionId }),
  getAiStatus: () => call("get_ai_status", none),
  getHooksStatus: () => call("get_hooks_status", none),
  installHooks: (dryRun: boolean) => call("install_hooks", { dryRun }),
  uninstallHooks: (dryRun: boolean) => call("uninstall_hooks", { dryRun }),
  getVoiceStatus: () => call("get_voice_status", none),
  testVoice: (args: Commands["test_voice"]["args"] = {}) => call("test_voice", args),
  setVoiceMuted: (muted: boolean) => call("set_voice_muted", { muted }),
  stopSpeech: () => call("stop_speech", none),
  speakOpening: (text: string) => call("speak_opening", { text }),
};

export const subscribe = {
  event: async (cb: (e: NyxEvent) => void) => (await backend()).listen<NyxEvent>(EVT_EVENT, cb),
  usage: async (cb: (u: UsageSummary) => void) => (await backend()).listen<UsageSummary>(EVT_USAGE, cb),
  delegation: async (cb: (d: Delegation) => void) => (await backend()).listen<Delegation>(EVT_DELEGATION, cb),
  chat: async (cb: (m: ChatMessage) => void) => (await backend()).listen<ChatMessage>(EVT_CHAT, cb),
  openTab: async (cb: (tab: OpenTab) => void) =>
    (await backend()).listen<{ tab: OpenTab }>(EVT_OPEN_TAB, (p) => cb(p.tab)),
  agents: async (cb: (a: AgentSession[]) => void) => (await backend()).listen<AgentSession[]>(EVT_AGENTS, cb),
  settings: async (cb: (s: Settings) => void) => (await backend()).listen<Settings>(EVT_SETTINGS, cb),
  speech: async (cb: (s: SpeechEvent) => void) => (await backend()).listen<SpeechEvent>(EVT_SPEECH, cb),
  speechEnd: async (cb: (e: { id: string; interrupted: boolean }) => void) =>
    (await backend()).listen<{ id: string; interrupted: boolean }>(EVT_SPEECH_END, cb),
};
