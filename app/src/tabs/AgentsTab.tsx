import { useEffect, useState } from "react";
import { api } from "../api/client";
import type { AgentSession, Snapshot } from "../api/contract";
import { Button, Card, EmptyState, StatusBadge, type CardTone } from "../components";
import { formatTokens, formatUsd, relativeTime, totalTokens } from "../lib/format";

const TONE: Record<AgentSession["status"], CardTone> = {
  running: "running", waiting: "waiting", error: "error", done: "default", idle: "idle", ended: "idle",
};

export function AgentsTab({ snap, now, onGoConfig }: { snap: Snapshot; now: number; onGoConfig: () => void }) {
  const noAgents = snap.agents.length === 0;
  const [hooksInstalled, setHooksInstalled] = useState<boolean | null>(null);
  useEffect(() => {
    if (noAgents) api.getHooksStatus().then((h) => setHooksInstalled(h.installed)).catch(() => setHooksInstalled(null));
  }, [noAgents]);
  const ctxName = (id: string | null) => snap.contexts.find((c) => c.id === id)?.name ?? "sem contexto";
  const order = { waiting: 0, running: 1, error: 2, done: 3, idle: 4, ended: 5 } as const;
  const agents = [...snap.agents].sort(
    (a, b) => order[a.status] - order[b.status] || Date.parse(b.updatedAt) - Date.parse(a.updatedAt),
  );
  if (agents.length === 0) {
    if (hooksInstalled === false) {
      return (
        <EmptyState title="Agentes" action={<Button onClick={onGoConfig}>Ir para Config</Button>}>
          Silêncio total, Dev. Sem os hooks eu não ouço o Claude Code.
        </EmptyState>
      );
    }
    return <EmptyState title="Agentes">Nenhum agente por aqui. Quando uma sessão do Claude Code abrir, ela aparece.</EmptyState>;
  }
  return (
    <ul className="m-0 flex list-none flex-col gap-2 p-0">
      {agents.map((a) => (
        <li key={a.sessionId}>
          <Card tone={TONE[a.status]}>
            <div className="flex items-baseline justify-between gap-2">
              <div className="min-w-0">
                <div className={`truncate font-display text-base leading-[1.1] font-semibold ${a.status === "idle" || a.status === "ended" ? "text-muted" : "text-text"}`} title={a.cwd ?? undefined}>
                  {a.folderName ?? "(sem pasta)"}
                </div>
                <div className="mt-0.5 text-[10px] tracking-[0.06em] text-muted">
                  {ctxName(a.contextId)} · <code className="tracking-[0.12em] uppercase">{a.sessionId.slice(0, 6)}</code>
                </div>
              </div>
              <StatusBadge status={a.status} />
            </div>
            {a.lastMessage && (
              <p className="mt-2 mb-0 bg-bg px-2 py-1.5 text-[11px] text-text">{a.lastMessage}</p>
            )}
            <dl className="mt-2 grid grid-cols-3 gap-2 border-t border-dashed border-line pt-[7px] text-[11px]">
              <div>
                <dt className="text-[10px] uppercase tracking-[0.12em] text-muted">tokens</dt>
                <dd className="m-0 mt-px font-medium" title={`in ${a.tokens.input} / out ${a.tokens.output} / cache ${a.tokens.cacheRead + a.tokens.cacheWrite}`}>
                  {formatTokens(totalTokens(a.tokens))}
                </dd>
              </div>
              <div>
                <dt className="text-[10px] uppercase tracking-[0.12em] text-muted">custo est.</dt>
                <dd className="m-0 mt-px font-medium">{formatUsd(a.costUsd)}</dd>
              </div>
              <div>
                <dt className="text-[10px] uppercase tracking-[0.12em] text-muted">atividade</dt>
                <dd className="m-0 mt-px font-medium">{relativeTime(a.updatedAt, now)}</dd>
              </div>
            </dl>
          </Card>
        </li>
      ))}
    </ul>
  );
}
