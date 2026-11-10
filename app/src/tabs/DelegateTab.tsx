import { useEffect, useMemo, useState } from "react";
import { api } from "../api/client";
import type { Delegation, DelegationDraft, ProjectContext, Snapshot } from "../api/contract";
import {
  Button, Card, type CardTone, CardTitle, ConfirmCard, DelegationBadge, EmptyState, Field, SegmentedBar, Select, Stepper,
  type StepperItem, TextArea, Toggle,
} from "../components";
import { addStep, applyPlan, emptyDraft, moveStep, normalizeDraft, removeStep, updateStep, validateDraft } from "../delegation/draft";
import {
  approvedCount, contextPercent, isActive, progressPercent, splitFailure, stepLabel, stepTitle,
} from "../delegation/status";
import { extractBlockedTool, withTool } from "../delegation/tools";
import { useAiStatus } from "../hooks/useAiStatus";
import { useDelegation } from "../hooks/useDelegation";
import { useDelegations } from "../hooks/useDelegations";
import { formatUsd, relativeTime } from "../lib/format";

type View = { kind: "list" } | { kind: "edit" } | { kind: "detail"; id: string };

/** Card border graduates with attention: neon when you are needed, primary while running, plain when done. */
const toneOf = (s: Delegation["status"]): CardTone =>
  s === "checkpoint" || s === "blocked" ? "waiting" : s === "failed" ? "error" : isActive(s) ? "running" : s === "done" ? "default" : "idle";

export function DelegateTab({ snap, now }: { snap: Snapshot; now: number }) {
  const [ctxId, setCtxId] = useState<string>(snap.contexts[0]?.id ?? "");
  const [view, setView] = useState<View>({ kind: "list" });
  const ctx = snap.contexts.find((c) => c.id === ctxId) ?? null;

  useEffect(() => {
    if (!snap.contexts.some((c) => c.id === ctxId)) setCtxId(snap.contexts[0]?.id ?? "");
  }, [snap.contexts, ctxId]);

  if (snap.contexts.length === 0) {
    return <EmptyState title="Delegar">Sem contexto, sem delegação. Crie um com pasta e permissões em Contextos.</EmptyState>;
  }

  if (view.kind === "detail") {
    return <DelegationView id={view.id} ctx={ctx} now={now} onBack={() => setView({ kind: "list" })} />;
  }
  if (view.kind === "edit" && ctx) {
    return <DraftEditor ctx={ctx} onCancel={() => setView({ kind: "list" })} onCreated={(id) => setView({ kind: "detail", id })} />;
  }
  return (
    <DelegationList
      contexts={snap.contexts} ctxId={ctxId} setCtxId={setCtxId} now={now}
      onNew={() => setView({ kind: "edit" })} onOpen={(id) => setView({ kind: "detail", id })}
    />
  );
}

/* ---------- list ---------- */

function DelegationList({ contexts, ctxId, setCtxId, now, onNew, onOpen }: {
  contexts: ProjectContext[]; ctxId: string; setCtxId: (id: string) => void; now: number;
  onNew: () => void; onOpen: (id: string) => void;
}) {
  const { delegations, loading, error } = useDelegations(ctxId, true);
  return (
    <div className="flex flex-col gap-2.5">
      <Select label="Contexto" value={ctxId} onChange={(e) => setCtxId(e.target.value)}>
        {contexts.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
      </Select>
      <div><Button variant="primary" onClick={onNew}>+ Nova delegação</Button></div>
      {error && <p className="m-0 text-neon" role="alert">{error}</p>}
      {!loading && delegations.length === 0 && (
        <EmptyState>Nenhuma delegação neste contexto. Escreva os passos e eu paro em cada checkpoint.</EmptyState>
      )}
      <ul className="m-0 flex list-none flex-col gap-2 p-0">
        {delegations.map((d) => (
          <li key={d.id}>
            <Card tone={toneOf(d.status)}>
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="truncate font-display text-base leading-[1.1] font-semibold">{d.title}</div>
                  <div className="mt-0.5 text-[10px] tracking-[0.06em] text-muted">
                    {approvedCount(d)}/{d.steps.length} passos · {formatUsd(d.costUsd)} · {relativeTime(d.updatedAt, now)}
                  </div>
                </div>
                <DelegationBadge status={d.status} />
              </div>
              <div className="mt-2"><Button size="sm" onClick={() => onOpen(d.id)}>Abrir</Button></div>
            </Card>
          </li>
        ))}
      </ul>
    </div>
  );
}

/* ---------- draft editor ---------- */

function DraftEditor({ ctx, onCancel, onCreated }: { ctx: ProjectContext; onCancel: () => void; onCreated: (id: string) => void }) {
  const { keyPresent } = useAiStatus();
  const [draft, setDraft] = useState<DelegationDraft>(() => ({ ...emptyDraft(ctx.id), useAi: ctx.aiEnabled }));
  const [goal, setGoal] = useState("");
  const [planning, setPlanning] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [touched, setTouched] = useState(false);

  const errors = useMemo(() => validateDraft(draft, ctx, { keyPresent }), [draft, ctx, keyPresent]);
  const set = (p: Partial<DelegationDraft>) => setDraft((d) => ({ ...d, ...p }));

  async function plan() {
    setPlanning(true); setErr(null);
    try {
      const steps = await api.planDelegation(ctx.id, goal);
      const hasText = draft.steps.some((s) => s.trim());
      setDraft((d) => ({ ...d, steps: applyPlan(d.steps, steps, hasText), title: d.title || goal.slice(0, 60) }));
    } catch (e) { setErr(e instanceof Error ? e.message : String(e)); }
    setPlanning(false);
  }

  async function create(start: boolean) {
    setTouched(true);
    if (errors.length) return;
    setBusy(true); setErr(null);
    try {
      const d = await api.createDelegation(normalizeDraft(draft));
      if (start) await api.startDelegation(d.id);
      onCreated(d.id);
    } catch (e) { setErr(e instanceof Error ? e.message : String(e)); setBusy(false); }
  }

  const titleErr = touched ? errors.find((e) => e.field === "title") : undefined;
  return (
    <form className="flex flex-col gap-2.5" onSubmit={(e) => { e.preventDefault(); void create(true); }}>
      <span className="text-[10px] uppercase tracking-[0.14em] text-primary">Nova delegação · {ctx.name}</span>
      <Field label="Título" value={draft.title} onChange={(e) => set({ title: e.target.value })} autoFocus maxLength={80}
        hint={titleErr?.message} aria-invalid={!!titleErr} />

      <Card>
        <CardTitle meta={keyPresent ? "Haiku" : "sem chave"}>Pedir plano à Nyx</CardTitle>
        <div className="mt-2 flex flex-col gap-1.5">
          <TextArea label="Objetivo" rows={2} value={goal} onChange={(e) => setGoal(e.target.value)}
            hint={keyPresent ? "Ela propõe os passos; você edita antes de iniciar." : "Defina NYX_ANTHROPIC_API_KEY para a Nyx propor o plano. Manual funciona sem."} />
          <div><Button size="sm" disabled={!keyPresent || !goal.trim() || planning} onClick={plan}>{planning ? "Planejando…" : "Pedir plano à Nyx"}</Button></div>
        </div>
      </Card>

      <div className="flex flex-col gap-2" role="group" aria-label="Passos">
        {draft.steps.map((s, i) => (
          <div key={i} className="flex flex-col gap-1">
            <TextArea label={`Passo ${i + 1}`} rows={2} value={s} onChange={(e) => set({ steps: updateStep(draft.steps, i, e.target.value) })} />
            <div className="flex gap-1.5">
              <Button size="sm" variant="ghost" disabled={i === 0} aria-label={`Subir passo ${i + 1}`} onClick={() => set({ steps: moveStep(draft.steps, i, i - 1) })}>↑</Button>
              <Button size="sm" variant="ghost" disabled={i === draft.steps.length - 1} aria-label={`Descer passo ${i + 1}`} onClick={() => set({ steps: moveStep(draft.steps, i, i + 1) })}>↓</Button>
              <Button size="sm" variant="ghost" disabled={draft.steps.length <= 1} aria-label={`Remover passo ${i + 1}`} onClick={() => set({ steps: removeStep(draft.steps, i) })}>remover</Button>
            </div>
          </div>
        ))}
        <div><Button size="sm" onClick={() => set({ steps: addStep(draft.steps) })}>+ Passo</Button></div>
      </div>

      <Toggle checked={draft.useAi} disabled={!keyPresent} onChange={(useAi) => set({ useAi })} label="Resumo de cada etapa com IA"
        hint={keyPresent ? "Haiku, opt-in por tarefa." : "Indisponível sem NYX_ANTHROPIC_API_KEY."} />

      {touched && errors.length > 0 && (
        <ul className="m-0 list-none p-0 text-neon" role="alert">{errors.map((e, i) => <li key={i}>{e.message}</li>)}</ul>
      )}
      {err && <p className="m-0 text-neon" role="alert">{err}</p>}
      <div className="flex gap-1.5">
        <Button type="submit" variant="primary" disabled={busy}>Criar e iniciar</Button>
        <Button disabled={busy} onClick={() => void create(false)}>Salvar rascunho</Button>
        <Button variant="ghost" onClick={onCancel}>Cancelar</Button>
      </div>
    </form>
  );
}

/* ---------- delegation screen ---------- */

function DelegationView({ id, ctx, now, onBack }: { id: string; ctx: ProjectContext | null; now: number; onBack: () => void }) {
  const { delegation: d, error, actionError, busy, actions, can } = useDelegation(id);
  const [mode, setMode] = useState<"none" | "adjust" | "cancel" | "merge">("none");
  const [feedback, setFeedback] = useState("");

  if (error) return <p className="m-0 text-neon" role="alert">{error}</p>;
  if (!d || !can) return <p className="m-0 text-muted">Carregando…</p>;

  const done = async (fn: Promise<Delegation | null>) => { if (await fn) { setMode("none"); setFeedback(""); } };
  const items: StepperItem[] = d.steps.map((s) => ({
    title: stepTitle(s.prompt),
    state: s.status,
    detail: (
      <div className="flex flex-col gap-1">
        {s.liveActivity && <span className="text-neon" aria-live="polite">▶ {s.liveActivity}</span>}
        {s.report && (
          <div>
            <div className="text-text">{s.report.title}</div>
            <ul className="m-0 list-none p-0">{s.report.lines.slice(0, 5).map((l, i) => <li key={i}>{l}</li>)}</ul>
            {s.report.truncated && <span>…</span>}
          </div>
        )}
        {s.aiSummary && <div><span className="text-neon">Nyx: </span>{s.aiSummary}</div>}
        {s.feedback.length > 0 && <div>Ajustes: {s.feedback.join(" · ")}</div>}
        {(s.costUsd > 0 || s.commit) && (
          <span>
            {formatUsd(s.costUsd)}{s.contextFill != null && ` · contexto ${Math.round(s.contextFill * 100)}%`}
            {s.attempts > 1 && ` · tentativa ${s.attempts}`}{s.commit && ` · commit ${s.commit}`}
          </span>
        )}
      </div>
    ),
  }));

  return (
    <div className="flex flex-col gap-2.5">
      <div><Button size="sm" variant="ghost" onClick={onBack}>← Delegações</Button></div>
      <Card tone={toneOf(d.status)}>
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <div className="font-display text-base leading-[1.1] font-semibold">{d.title}</div>
            <div className="mt-0.5 break-all text-[10px] tracking-[0.04em] text-muted">{d.branch} ← {d.baseBranch}</div>
          </div>
          <DelegationBadge status={d.status} />
        </div>
        <div className="mt-2 flex justify-between text-[10px] uppercase tracking-[0.1em] text-muted">
          <span>{stepLabel(d)} · {progressPercent(d)}%</span>
          <span>{formatUsd(d.costUsd)}{d.handoffs > 0 && ` · ${d.handoffs} ${d.handoffs === 1 ? "passagem" : "passagens"}`}</span>
        </div>
        <div className="mt-1"><SegmentedBar percent={contextPercent(d)} label="Contexto do passo atual" marks={[60]} /></div>
        <div className="mt-0.5 text-[10px] text-muted">janela de contexto {contextPercent(d) ?? "--"}% · passagem em 60% · {relativeTime(d.updatedAt, now)}</div>
      </Card>

      {d.status === "handoff" && (
        <Card tone="primary"><p className="m-0 text-[11px]">Passagem de sessão: o contexto passou de 60%. Peço o PASSAGEM.md ao agente e abro uma sessão nova que começa por ele.</p></Card>
      )}
      {d.status === "blocked" && (
        <BlockedCard d={d} ctx={ctx} busy={busy} onRedo={(fb) => done(actions.adjust(fb))} onApprove={() => done(actions.approve())} onCancel={() => setMode("cancel")} />
      )}
      {d.status === "failed" && <FailureCard reason={d.blockedReason} />}
      {actionError && <p className="m-0 text-neon" role="alert">{actionError}</p>}

      <Stepper items={items} current={d.currentStep} />

      {mode === "cancel" ? (
        <ConfirmCard tone="danger" confirmLabel="Cancelar delegação" busy={busy} onCancel={() => setMode("none")} label="Cancelar delegação"
          onConfirm={() => void done(actions.cancel())}>
          Mata o processo e mantém o worktree e a branch {d.branch}. Não dá para retomar.
        </ConfirmCard>
      ) : mode === "merge" ? (
        <ConfirmCard confirmLabel={`Mesclar em ${d.baseBranch}`} busy={busy} onCancel={() => setMode("none")} label="Mesclar branch"
          onConfirm={() => void done(actions.finish(true))}>
          Merge --no-ff de {d.branch} em {d.baseBranch}, remove o worktree e mantém a branch.
        </ConfirmCard>
      ) : mode === "adjust" ? (
        <form className="flex flex-col gap-1.5" onSubmit={(e) => { e.preventDefault(); if (feedback.trim()) void done(actions.adjust(feedback)); }}>
          <TextArea label="Ajuste para este passo" rows={3} value={feedback} onChange={(e) => setFeedback(e.target.value)} autoFocus
            hint="O agente refaz o mesmo passo, na mesma sessão, com isso." />
          <div className="flex gap-1.5">
            <Button type="submit" variant="primary" disabled={busy || !feedback.trim()}>Enviar ajuste</Button>
            <Button variant="ghost" onClick={() => setMode("none")}>Voltar</Button>
          </div>
        </form>
      ) : (
        <div className="flex flex-wrap gap-1.5">
          {can.start && <Button variant="primary" disabled={busy} onClick={() => void actions.start()}>Iniciar</Button>}
          {can.approve && <Button variant="primary" disabled={busy} onClick={() => void actions.approve()}>Aprovar</Button>}
          {can.retryApprove && <Button variant="primary" disabled={busy} onClick={() => void actions.approve()}>Tentar aprovar de novo</Button>}
          {can.adjust && d.status !== "blocked" && <Button disabled={busy} onClick={() => setMode("adjust")}>{d.status === "failed" ? "Refazer com ajuste" : "Ajustar"}</Button>}
          {can.cancel && <Button variant="ghost" disabled={busy} onClick={() => setMode("cancel")}>Cancelar</Button>}
          {can.finish && can.merge && <Button variant="primary" disabled={busy} onClick={() => setMode("merge")}>Finalizar: mesclar</Button>}
          {can.finish && <Button disabled={busy} onClick={() => void actions.finish(false)}>Manter a branch</Button>}
        </div>
      )}
    </div>
  );
}

function FailureCard({ reason }: { reason: string | null }) {
  const { summary, output } = splitFailure(reason);
  return (
    <Card tone="error">
      <CardTitle meta="git / agente">Falhou</CardTitle>
      <p className="mt-1.5 mb-0 text-[11px]" role="alert">{summary}</p>
      {output && <pre className="mt-1.5 mb-0 max-h-40 overflow-auto whitespace-pre-wrap border border-line bg-bg p-2 font-mono text-[11px] text-muted">{output}</pre>}
    </Card>
  );
}

function BlockedCard({ d, ctx, busy, onRedo, onApprove, onCancel }: {
  d: Delegation; ctx: ProjectContext | null; busy: boolean; onRedo: (feedback: string) => void; onApprove: () => void; onCancel: () => void;
}) {
  const [confirmingApprove, setConfirmingApprove] = useState(false);
  const [tool, setTool] = useState(() => extractBlockedTool(d.blockedReason) ?? "");
  const [err, setErr] = useState<string | null>(null);

  async function addAndRedo() {
    if (!ctx || !tool.trim()) return;
    try {
      await api.saveContext({ ...ctx, allowedTools: withTool(ctx.allowedTools, tool) });
      onRedo(`Ferramenta liberada no contexto: ${tool.trim()}. Refaça o passo.`);
    } catch (e) { setErr(e instanceof Error ? e.message : String(e)); }
  }

  return (
    <Card tone="waiting">
      <CardTitle meta="permissão">Bloqueado</CardTitle>
      <p className="mt-1.5 mb-0 text-[11px]">{d.blockedReason ?? "O passo precisou de uma ferramenta fora da lista do contexto."}</p>
      <div className="mt-2 flex flex-col gap-1.5">
        <Field label="Ferramenta a liberar" value={tool} onChange={(e) => setTool(e.target.value)} spellCheck={false}
          hint={`Entra na lista de ${ctx?.name ?? "contexto"} e vale para as próximas delegações.`} />
        {err && <p className="m-0 text-neon" role="alert">{err}</p>}
        {confirmingApprove ? (
          <ConfirmCard confirmLabel="Aprovar assim mesmo" busy={busy} label="Aprovar assim mesmo"
            onConfirm={onApprove} onCancel={() => setConfirmingApprove(false)}>
            O passo foi interrompido e pode estar incompleto. Aprovar faz o commit do que existe agora e segue para o próximo passo.
          </ConfirmCard>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            <Button variant="primary" disabled={busy || !tool.trim() || !ctx} onClick={() => void addAndRedo()}>Adicionar ao contexto e refazer</Button>
            <Button disabled={busy} onClick={() => setConfirmingApprove(true)}>Aprovar assim mesmo</Button>
            <Button variant="ghost" onClick={onCancel}>Cancelar</Button>
          </div>
        )}
      </div>
    </Card>
  );
}
