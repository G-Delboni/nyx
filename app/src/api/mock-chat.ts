// Mock of the Nyx chat (docs/specs/NOITE-2.md §4): replies in character with exactly one
// proposed action. Nothing runs until the user confirms the card (run_chat_action).
import type { ChatAction, ChatMessage } from "./contract";
import { EVT_CHAT } from "./contract";

export interface ChatMockDeps {
  now: () => number;
  emit: (name: string, payload: unknown) => void;
  keyPresent: () => boolean;
  /** Runs the same command the UI would run; throws on failure. */
  runAction: (action: ChatAction, contextId: string | null) => Promise<void>;
  /** Simulated latency of Haiku; 0 in tests. */
  replyDelayMs: number;
}

export interface ChatMock {
  get(contextId: string | null, limit: number): ChatMessage[];
  send(contextId: string | null, text: string): Promise<ChatMessage>;
  clear(contextId: string | null): void;
  runAction(messageId: number, actionId: string): Promise<ChatMessage>;
  /** Cost accumulated by Nyx's own replies (feeds AiStatus.weekCostUsd). */
  totalCost(): number;
}

export function pickReply(text: string, contextId: string | null): { text: string; action: Omit<ChatAction, "id" | "state"> } {
  const t = text.toLowerCase();
  if (/hook|instal/.test(t)) {
    return {
      text: "Os hooks ainda não estão instalados, Dev. Posso instalar com backup do settings.",
      action: { kind: "installHooks", label: "Instalar hooks", args: { dryRun: false } },
    };
  }
  if (/delega|fazer|implement|criar|tarefa/.test(t)) {
    return {
      text: "Dá para fazer em 3 passos curtos. Cada um para no checkpoint.",
      action: {
        kind: "startDelegation", label: "Iniciar delegação (3 passos)",
        args: { contextId, title: text.slice(0, 60), steps: ["Levantar o estado atual", "Implementar a mudança", "Rodar testes e build"] },
      },
    };
  }
  if (/aprov/.test(t)) {
    return { text: "Há um checkpoint esperando. Aprovo o passo atual?", action: { kind: "approveStep", label: "Aprovar passo atual", args: {} } };
  }
  if (/uso|semana|teto|custo/.test(t)) {
    return { text: "Semana em 70% do teto. Faltam 3 dias.", action: { kind: "openPanel", label: "Abrir aba Uso", args: { tab: "usage" } } };
  }
  return { text: "Online, Dev. Dois agentes ativos e nada pegando fogo.", action: { kind: "openPanel", label: "Abrir aba Agentes", args: { tab: "agents" } } };
}

export function createChatMock(deps: ChatMockDeps): ChatMock {
  let nextId = 1;
  let nextAction = 1;
  let cost = 0;
  const messages: ChatMessage[] = [];
  const iso = () => new Date(deps.now()).toISOString();
  const same = (a: string | null, b: string | null) => (a ?? null) === (b ?? null);
  const publish = (m: ChatMessage) => { deps.emit(EVT_CHAT, structuredClone(m)); return structuredClone(m); };

  return {
    get: (contextId, limit) =>
      structuredClone(messages.filter((m) => same(m.contextId, contextId)).slice(-limit)),
    async send(contextId, text) {
      if (!deps.keyPresent()) throw new Error("IA indisponível: defina NYX_ANTHROPIC_API_KEY.");
      const clean = text.trim();
      if (!clean) throw new Error("Mensagem vazia.");
      store({ id: nextId++, contextId, role: "user", text: clean, actions: [], costUsd: 0, createdAt: iso() });
      if (deps.replyDelayMs > 0) await new Promise((r) => setTimeout(r, deps.replyDelayMs));
      const r = pickReply(clean, contextId);
      const m: ChatMessage = {
        id: nextId++, contextId, role: "nyx", text: r.text, costUsd: 0.0008, createdAt: iso(),
        actions: [{ ...r.action, id: `a${nextAction++}`, state: "proposed" }],
      };
      cost += m.costUsd;
      return store(m);
    },
    clear(contextId) {
      for (let i = messages.length - 1; i >= 0; i--) if (same(messages[i].contextId, contextId)) messages.splice(i, 1);
    },
    async runAction(messageId, actionId) {
      const m = messages.find((x) => x.id === messageId);
      const action = m?.actions.find((a) => a.id === actionId);
      if (!m || !action) throw new Error("Ação não encontrada.");
      if (action.state !== "proposed") throw new Error("Esta ação já foi tratada.");
      try {
        await deps.runAction(structuredClone(action), m.contextId);
        action.state = "done";
      } catch {
        action.state = "failed";
      }
      return publish(m);
    },
    totalCost: () => +cost.toFixed(4),
  };

  function store(m: ChatMessage): ChatMessage {
    messages.push(m);
    return publish(m);
  }
}
