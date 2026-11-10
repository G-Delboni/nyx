// Mock of the delegation engine (docs/specs/NOITE-2.md §2). Steps advance on their own with
// timers: running -> checkpoint -> approve -> next step. Prompt markers pick the scenario:
//   "[passagem]" -> step ends with context fill >= 0.6, so approving triggers a hand-off;
//   "[bloqueio]" -> step needs a tool outside the context allowlist and ends "blocked".
import type { Delegation, DelegationDraft, DelegationStep } from "./contract";
import { EVT_DELEGATION } from "./contract";

export const BLOCKED_TOOL = "Bash(git push:*)";

/** Commit failures the core can hit on approve; first line = summary, rest = raw git output. */
export const GIT_FAILURES: Record<string, string> = {
  "[sem-commits]":
    "O repositório não tem commits. Faça um commit inicial na branch base e tente de novo.\n" +
    "fatal: ambiguous argument 'HEAD': unknown revision or path not in the working tree.",
  "[sem-identidade]":
    "O git não sabe quem você é. Defina user.name e user.email e tente de novo.\n" +
    "Author identity unknown\n*** Please tell me who you are.\n  git config --global user.email \"you@example.com\"\n  git config --global user.name \"Your Name\"",
  "[pre-commit]":
    "O hook pre-commit falhou. Corrija o que ele reclamou e aprove de novo.\n" +
    "> eslint .\n  src/main.ts\n    3:7  error  'x' is assigned a value but never used  no-unused-vars\npre-commit hook exited with code 1",
};
const ACTIVE = new Set(["running", "checkpoint", "handoff", "blocked"]);
const FINISHED = new Set(["done", "cancelled"]);

export interface DelegationMockDeps {
  now: () => number;
  emit: (name: string, payload: unknown) => void;
  allowedTools: (contextId: string) => string[];
  contextExists: (contextId: string) => boolean;
  keyPresent: () => boolean;
  /** Duration of one simulated step; 0 disables timers (tests drive `finishStep`). */
  stepMs: number;
}

export interface DelegationMock {
  list(contextId?: string | null, includeFinished?: boolean): Delegation[];
  get(id: string): Delegation;
  create(draft: DelegationDraft): Delegation;
  plan(contextId: string, goal: string): string[];
  start(id: string): Delegation;
  approve(id: string): Delegation;
  adjust(id: string, feedback: string): Delegation;
  cancel(id: string): Delegation;
  finish(id: string, merge: boolean): Delegation;
  /** Ends the running step now (what the timer does). */
  finishStep(id: string): Delegation;
  /** Ends the PASSAGEM.md hand-off now and starts the next step in the fresh session. */
  completeHandoff(id: string): Delegation;
  activeCount(): number;
  stop(): void;
}

const ACTIVITY = ["Lendo arquivos do projeto", "Editando src/main.rs", "Rodando testes"];

export function createDelegationMock(deps: DelegationMockDeps): DelegationMock {
  const iso = () => new Date(deps.now()).toISOString();
  const items = new Map<string, Delegation>();
  const useAi = new Map<string, boolean>();
  const timers = new Map<string, Array<ReturnType<typeof setTimeout>>>();
  const gitFailed = new Set<string>(); // each marker fails once per step, then the retry works
  let counter = 0;
  let commitCounter = 0;

  const blankStep = (index: number, prompt: string): DelegationStep => ({
    index, prompt, status: "pending", attempts: 0, report: null, aiSummary: null, feedback: [],
    commit: null, costUsd: 0, contextFill: null, liveActivity: null, startedAt: null, finishedAt: null,
  });

  const clone = (d: Delegation): Delegation => structuredClone(d);
  const need = (id: string): Delegation => {
    const d = items.get(id);
    if (!d) throw new Error(`Delegação ${id} não existe.`);
    return d;
  };
  const commit = (d: Delegation): Delegation => {
    d.updatedAt = iso();
    const out = clone(d);
    deps.emit(EVT_DELEGATION, out);
    return out;
  };
  const clearTimers = (id: string) => {
    (timers.get(id) ?? []).forEach(clearTimeout);
    timers.delete(id);
  };
  const later = (id: string, ms: number, fn: () => void) => {
    const t = setTimeout(fn, ms);
    timers.set(id, [...(timers.get(id) ?? []), t]);
  };

  function runStep(d: Delegation) {
    const step = d.steps[d.currentStep];
    step.status = "running";
    step.attempts += 1;
    step.startedAt = iso();
    step.finishedAt = null;
    step.report = null;
    step.aiSummary = null;
    step.liveActivity = ACTIVITY[0];
    d.status = "running";
    d.blockedReason = null;
    clearTimers(d.id);
    if (deps.stepMs > 0) {
      ACTIVITY.slice(1).forEach((label, i) =>
        later(d.id, (deps.stepMs * (i + 1)) / ACTIVITY.length, () => {
          const cur = items.get(d.id);
          if (cur?.status === "running") { cur.steps[cur.currentStep].liveActivity = label; commit(cur); }
        }));
      later(d.id, deps.stepMs, () => { if (items.get(d.id)?.status === "running") finishStep(d.id); });
    }
  }

  function finishStep(id: string): Delegation {
    const d = need(id);
    if (d.status !== "running") throw new Error("Nenhum passo em execução.");
    clearTimers(id);
    const i = d.currentStep;
    const step = d.steps[i];
    step.liveActivity = null;
    step.finishedAt = iso();
    if (step.prompt.includes("[bloqueio]") && !deps.allowedTools(d.contextId).includes(BLOCKED_TOOL)) {
      step.status = "failed";
      d.status = "blocked";
      d.blockedReason = `O passo precisa de ${BLOCKED_TOOL}, que não está na lista do contexto.`;
      return commit(d);
    }
    const cost = +(0.28 + i * 0.07).toFixed(2);
    step.status = "checkpoint";
    step.costUsd = +(step.costUsd + cost).toFixed(2);
    d.costUsd = +d.steps.reduce((s, x) => s + x.costUsd, 0).toFixed(2);
    step.contextFill = step.prompt.includes("[passagem]") ? 0.63 : +Math.min(0.55, 0.18 + 0.12 * i).toFixed(2);
    step.report = {
      title: `Passo ${i + 1} de ${d.steps.length} concluído`,
      lines: ["3 arquivos alterados.", "Testes passando.", "Nenhum aviso novo."],
      truncated: false,
    };
    if (useAi.get(id) && deps.keyPresent()) {
      step.aiSummary = "Ajustei os arquivos pedidos e os testes passam. Nada fora do escopo foi tocado.";
    }
    d.status = "checkpoint";
    return commit(d);
  }

  function completeHandoff(id: string): Delegation {
    const d = need(id);
    if (d.status !== "handoff") throw new Error("Nenhuma passagem em andamento.");
    d.handoffs += 1;
    d.claudeSessionId = `sess-${id}-h${d.handoffs}`;
    runStep(d);
    return commit(d);
  }

  const mock: DelegationMock = {
    list(contextId, includeFinished = false) {
      return [...items.values()]
        .filter((d) => !contextId || d.contextId === contextId)
        .filter((d) => includeFinished || !FINISHED.has(d.status))
        .sort((a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt))
        .map(clone);
    },
    get: (id) => clone(need(id)),
    create(draft) {
      if (!deps.contextExists(draft.contextId)) throw new Error("Contexto não encontrado.");
      const steps = draft.steps.map((s) => s.trim()).filter(Boolean);
      if (!draft.title.trim()) throw new Error("Dê um título à delegação.");
      if (steps.length === 0) throw new Error("A delegação precisa de pelo menos um passo.");
      const id = `d-${(++counter).toString(36)}${Math.random().toString(36).slice(2, 5)}`;
      const d: Delegation = {
        id, contextId: draft.contextId, title: draft.title.trim(), status: "draft", currentStep: 0,
        steps: steps.map((p, i) => blankStep(i, p)),
        worktreePath: `C:\\Users\\dev\\Documents\\nyx\\.nyx\\worktrees\\${id}`,
        branch: `nyx/deleg-${id}`, baseBranch: "main", claudeSessionId: null, handoffs: 0,
        blockedReason: null, costUsd: 0, createdAt: iso(), updatedAt: iso(),
      };
      items.set(id, d);
      useAi.set(id, draft.useAi);
      return commit(d);
    },
    plan(contextId, goal) {
      if (!deps.keyPresent()) throw new Error("IA indisponível: defina NYX_ANTHROPIC_API_KEY.");
      if (!deps.contextExists(contextId)) throw new Error("Contexto não encontrado.");
      const g = goal.trim();
      if (!g) throw new Error("Descreva o objetivo.");
      return [
        `Levantar o estado atual relacionado a: ${g}`,
        `Implementar a mudança principal: ${g}`,
        "Rodar testes e build e corrigir o que quebrar",
        "Resumir o que mudou em até 5 linhas",
      ];
    },
    start(id) {
      const d = need(id);
      if (d.status !== "draft") throw new Error("Só dá para iniciar um rascunho.");
      const clash = [...items.values()].some((o) => o.id !== id && o.contextId === d.contextId && ACTIVE.has(o.status));
      if (clash) throw new Error("Já existe uma delegação ativa neste contexto.");
      d.claudeSessionId = `sess-${d.id}`;
      d.currentStep = 0;
      runStep(d);
      return commit(d);
    },
    approve(id) {
      const d = need(id);
      const retry = d.status === "failed" && d.steps[d.currentStep].status === "checkpoint";
      if (d.status !== "checkpoint" && d.status !== "blocked" && !retry) throw new Error("Não há checkpoint para aprovar.");
      const step = d.steps[d.currentStep];
      const marker = Object.keys(GIT_FAILURES).find((m) => step.prompt.includes(m));
      if (marker && !gitFailed.has(`${id}:${d.currentStep}`)) {
        gitFailed.add(`${id}:${d.currentStep}`);
        step.status = "checkpoint";
        d.status = "failed";
        d.blockedReason = GIT_FAILURES[marker];
        return commit(d);
      }
      d.blockedReason = null;
      step.status = "approved";
      step.commit = (0x1a2b3c0 + ++commitCounter * 0x111).toString(16).slice(0, 7).padStart(7, "0");
      if (d.currentStep >= d.steps.length - 1) {
        d.status = "done";
        return commit(d);
      }
      d.currentStep += 1;
      if ((step.contextFill ?? 0) >= 0.6) {
        d.status = "handoff";
        commit(d);
        if (deps.stepMs > 0) later(id, deps.stepMs / 2, () => { if (items.get(id)?.status === "handoff") completeHandoff(id); });
        return clone(d);
      }
      runStep(d);
      return commit(d);
    },
    adjust(id, feedback) {
      const d = need(id);
      if (!["checkpoint", "blocked", "failed"].includes(d.status)) throw new Error("Este passo não pode ser ajustado agora.");
      const text = feedback.trim();
      if (!text) throw new Error("Escreva o ajuste.");
      d.steps[d.currentStep].feedback.push(text);
      runStep(d);
      return commit(d);
    },
    cancel(id) {
      const d = need(id);
      if (FINISHED.has(d.status)) throw new Error("A delegação já terminou.");
      clearTimers(id);
      d.status = "cancelled";
      d.steps.forEach((s) => { s.liveActivity = null; });
      return commit(d);
    },
    finish(id, merge) {
      const d = need(id);
      if (d.status !== "done" && d.status !== "cancelled") throw new Error("Só dá para finalizar uma delegação concluída ou cancelada.");
      if (merge && d.status === "cancelled") throw new Error("Delegação cancelada não é mesclada. Mantenha a branch.");
      return commit(d);
    },
    finishStep,
    completeHandoff,
    activeCount: () => [...items.values()].filter((d) => ACTIVE.has(d.status)).length,
    stop() { [...timers.keys()].forEach(clearTimers); },
  };

  seed();
  return mock;

  function seed() {
    const mk = (over: Partial<Delegation> & Pick<Delegation, "id" | "contextId" | "title">, steps: DelegationStep[]) => {
      const d: Delegation = {
        status: "done", currentStep: 0, steps, worktreePath: `C:\\Users\\dev\\Documents\\nyx\\.nyx\\worktrees\\${over.id}`,
        branch: `nyx/deleg-${over.id}`, baseBranch: "main", claudeSessionId: `sess-${over.id}`, handoffs: 0,
        blockedReason: null, costUsd: 0, createdAt: new Date(deps.now() - 3_600_000).toISOString(),
        updatedAt: new Date(deps.now() - 600_000).toISOString(), ...over,
      };
      d.costUsd = +d.steps.reduce((s, x) => s + x.costUsd, 0).toFixed(2);
      items.set(d.id, d);
    };
    const done = (i: number, prompt: string, hash: string): DelegationStep => ({
      ...blankStep(i, prompt), status: "approved", attempts: 1, commit: hash, costUsd: 0.31, contextFill: 0.2 + i * 0.1,
      report: { title: `Passo ${i + 1} concluído`, lines: ["2 arquivos alterados.", "Testes passando."], truncated: false },
    });
    mk({ id: "d-demo1", contextId: "ctx-nyx", title: "Painel de uso por contexto", status: "checkpoint", currentStep: 1 }, [
      done(0, "Ler o contrato e listar os campos de uso por contexto", "a1b2c3d"),
      {
        ...blankStep(1, "Implementar a quebra por contexto na aba Uso"), status: "checkpoint", attempts: 1, costUsd: 0.44,
        contextFill: 0.31, report: { title: "Passo 2 de 3 concluído", lines: ["Aba Uso agrupa por contexto.", "3 arquivos alterados.", "Build passando."], truncated: false },
        aiSummary: null,
      },
      blankStep(2, "Cobrir com testes e atualizar as notas"),
    ]);
    mk({ id: "d-demo0", contextId: "ctx-demo", title: "Revisar roteiros da semana", status: "done", currentStep: 1 }, [
      done(0, "Listar os roteiros pendentes", "9f8e7d6"),
      done(1, "Corrigir a ortografia e padronizar os títulos", "5c4b3a2"),
    ]);
  }
}
