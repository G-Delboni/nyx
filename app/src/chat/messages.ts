// Pure chat helpers.
import type { ChatAction, ChatMessage } from "../api/contract";

export const MAX_CHAT_CHARS = 2000;

export type SendBlock = "empty" | "tooLong" | "noKey" | "sending" | null;

export function sendBlock(text: string, o: { keyPresent: boolean; sending: boolean }): SendBlock {
  const t = text.trim();
  if (!o.keyPresent) return "noKey";
  if (o.sending) return "sending";
  if (!t) return "empty";
  if (t.length > MAX_CHAT_CHARS) return "tooLong";
  return null;
}

/** Insert or replace by id, keeping chronological order (by id). */
export function mergeMessage(list: ChatMessage[], m: ChatMessage): ChatMessage[] {
  const i = list.findIndex((x) => x.id === m.id);
  const next = i >= 0 ? list.map((x) => (x.id === m.id ? m : x)) : [...list, m];
  return next.sort((a, b) => a.id - b.id);
}

export function inContext(m: ChatMessage, contextId: string | null): boolean {
  return (m.contextId ?? null) === (contextId ?? null);
}

export const ACTION_STATE_LABEL: Record<ChatAction["state"], string> = {
  proposed: "proposta",
  done: "feito",
  failed: "falhou",
  dismissed: "dispensada",
};

export function pendingActions(messages: ChatMessage[]): Array<{ message: ChatMessage; action: ChatAction }> {
  return messages.flatMap((message) =>
    message.actions.filter((a) => a.state === "proposed").map((action) => ({ message, action })));
}

/** Total API spend of Nyx's replies in the list. */
export function replyCost(messages: ChatMessage[]): number {
  return +messages.reduce((s, m) => s + (m.role === "nyx" ? m.costUsd : 0), 0).toFixed(6);
}

export const NO_KEY_HELP =
  "Sem chave, sem conversa, Dev. Defina NYX_ANTHROPIC_API_KEY nas variáveis de ambiente do Windows e reinicie a Nyx.";

/** Plain sentence shown on the confirmation card of a proposed action. */
export function describeAction(a: ChatAction): string {
  const x = a.args;
  switch (a.kind) {
    case "startDelegation": {
      const n = Array.isArray(x.steps) ? x.steps.length : 0;
      return `Criar e iniciar a delegação "${String(x.title ?? "sem título")}"${n ? ` com ${n} passos` : ""}. Cada passo para no checkpoint.`;
    }
    case "approveStep": return "Aprovar o passo em checkpoint. Isso faz o commit e segue para o próximo.";
    case "adjustStep": return "Pedir ajuste do passo atual com o feedback proposto.";
    case "installHooks": return "Instalar os hooks do Claude Code no settings.json, com backup antes.";
    case "createContext": return `Criar o contexto "${String(x.name ?? "novo")}".`;
    case "openPanel": return "Abrir a aba indicada no painel.";
  }
}
