// Pure logic for the delegation draft editor.
import type { DelegationDraft, ProjectContext } from "../api/contract";

export const MAX_STEPS = 12;
export const MAX_TITLE = 80;
export const MAX_STEP_CHARS = 4000;

export interface DraftError {
  field: "context" | "title" | "steps" | "tools";
  /** Index of the offending step, when the error is about one step. */
  step?: number;
  message: string;
}

export function emptyDraft(contextId = ""): DelegationDraft {
  return { contextId, title: "", steps: [""], useAi: false, model: null };
}

export function validateDraft(
  draft: DelegationDraft,
  ctx: Pick<ProjectContext, "allowedTools"> | null | undefined,
  ai: { keyPresent: boolean } = { keyPresent: false },
): DraftError[] {
  const errors: DraftError[] = [];
  if (!draft.contextId || !ctx) errors.push({ field: "context", message: "Escolha um contexto." });
  else if (ctx.allowedTools.length === 0) {
    errors.push({ field: "tools", message: "O contexto não tem ferramentas permitidas. Defina a lista em Contextos." });
  }
  const title = draft.title.trim();
  if (!title) errors.push({ field: "title", message: "Dê um título." });
  else if (title.length > MAX_TITLE) errors.push({ field: "title", message: `Título com no máximo ${MAX_TITLE} caracteres.` });
  const steps = draft.steps.map((s) => s.trim());
  const filled = steps.filter(Boolean);
  if (filled.length === 0) errors.push({ field: "steps", message: "Escreva pelo menos um passo." });
  if (steps.length > MAX_STEPS) errors.push({ field: "steps", message: `No máximo ${MAX_STEPS} passos.` });
  steps.forEach((s, i) => {
    if (s.length > MAX_STEP_CHARS) {
      errors.push({ field: "steps", step: i, message: `Passo ${i + 1} passa de ${MAX_STEP_CHARS} caracteres.` });
    }
  });
  if (draft.useAi && !ai.keyPresent) {
    errors.push({ field: "steps", message: "IA ligada, mas a NYX_ANTHROPIC_API_KEY não foi encontrada." });
  }
  return errors;
}

/** What gets sent to create_delegation: trimmed, empty steps dropped. */
export function normalizeDraft(draft: DelegationDraft): DelegationDraft {
  return {
    ...draft,
    title: draft.title.trim(),
    steps: draft.steps.map((s) => s.trim()).filter(Boolean),
  };
}

export function moveStep<T>(steps: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= steps.length || to >= steps.length) return steps;
  const next = [...steps];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

export function addStep(steps: string[], at: number = steps.length, text = ""): string[] {
  if (steps.length >= MAX_STEPS) return steps;
  const next = [...steps];
  next.splice(Math.max(0, Math.min(at, steps.length)), 0, text);
  return next;
}

export function removeStep(steps: string[], index: number): string[] {
  if (steps.length <= 1 || index < 0 || index >= steps.length) return steps;
  return steps.filter((_, i) => i !== index);
}

export function updateStep(steps: string[], index: number, text: string): string[] {
  if (index < 0 || index >= steps.length) return steps;
  return steps.map((s, i) => (i === index ? text : s));
}

/** Replace the draft steps by an AI plan, keeping steps the user already wrote when asked. */
export function applyPlan(current: string[], plan: string[], keepExisting: boolean): string[] {
  const mine = keepExisting ? current.map((s) => s.trim()).filter(Boolean) : [];
  return [...mine, ...plan].slice(0, MAX_STEPS);
}
