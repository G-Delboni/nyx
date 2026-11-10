// Pure status helpers for a running Delegation.
import type { Delegation, DelegationStatus } from "../api/contract";

export const STATUS_LABEL: Record<DelegationStatus, string> = {
  draft: "RASCUNHO",
  running: "EXECUTANDO",
  checkpoint: "AGUARDANDO APROVAÇÃO",
  handoff: "PASSAGEM DE SESSÃO",
  blocked: "BLOQUEADO",
  failed: "FALHOU",
  done: "CONCLUÍDA",
  cancelled: "CANCELADA",
};

export const isActive = (s: DelegationStatus) =>
  s === "running" || s === "checkpoint" || s === "handoff" || s === "blocked";
export const isFinished = (s: DelegationStatus) => s === "done" || s === "cancelled";

export interface DelegationActions {
  start: boolean;
  approve: boolean;
  adjust: boolean;   // also redoes a blocked/failed step
  cancel: boolean;
  finish: boolean;   // merge / keep branch
  merge: boolean;    // merge is only offered when the work finished
  approveAnyway: boolean; // blocked: approve what exists, accepting the step may be incomplete
  retryApprove: boolean;  // failed while committing an approved step (git problem): try again
}

export function availableActions(d: Pick<Delegation, "status"> & Partial<Pick<Delegation, "currentStep" | "steps">>): DelegationActions {
  const s = d.status;
  return {
    start: s === "draft",
    approve: s === "checkpoint",
    adjust: s === "checkpoint" || s === "blocked" || s === "failed",
    cancel: s === "running" || s === "checkpoint" || s === "handoff" || s === "blocked" || s === "failed",
    finish: s === "done" || s === "cancelled",
    merge: s === "done",
    approveAnyway: s === "blocked",
    retryApprove: s === "failed" && d.steps?.[d.currentStep ?? 0]?.status === "checkpoint",
  };
}

/** "Etapa 2 de 4". */
export function stepLabel(d: Pick<Delegation, "currentStep" | "steps">): string {
  return `Etapa ${Math.min(d.currentStep + 1, d.steps.length)} de ${d.steps.length}`;
}

export function approvedCount(d: Pick<Delegation, "steps">): number {
  return d.steps.filter((s) => s.status === "approved").length;
}

export function progressPercent(d: Pick<Delegation, "steps">): number {
  return d.steps.length === 0 ? 0 : Math.round((approvedCount(d) / d.steps.length) * 100);
}

/** Context window usage of the current step, as 0..100 (null while unknown). */
export function contextPercent(d: Pick<Delegation, "currentStep" | "steps">): number | null {
  const f = d.steps[d.currentStep]?.contextFill;
  return f == null ? null : Math.round(f * 100);
}

/** Upsert a delegation received from nyx://delegation, newest first. */
export function upsertDelegation(list: Delegation[], d: Delegation): Delegation[] {
  const i = list.findIndex((x) => x.id === d.id);
  const next = i >= 0 ? list.map((x) => (x.id === d.id ? d : x)) : [d, ...list];
  return next.sort((a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt));
}

/** Short step title from its prompt: first line, trimmed, capped. */
export function stepTitle(prompt: string, max = 64): string {
  const first = prompt.split(/\r?\n/).find((l) => l.trim())?.trim() ?? "(passo vazio)";
  return first.length > max ? `${first.slice(0, max - 1)}…` : first;
}

/** Splits a failure text into a human summary and the raw tool output (shown in mono). */
export function splitFailure(reason: string | null | undefined): { summary: string; output: string | null } {
  if (!reason) return { summary: "O passo falhou.", output: null };
  const i = reason.indexOf("\n");
  if (i < 0) return { summary: reason, output: null };
  const output = reason.slice(i + 1).trim();
  return { summary: reason.slice(0, i).trim(), output: output || null };
}
