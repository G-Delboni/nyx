import type { ReactNode } from "react";
import type { DelegationStatus, SessionStatus } from "../api/contract";

/** Status uses icon + text + violet intensity (no red/green/yellow). */
const SESSION: Record<SessionStatus, { label: string; icon: ReactNode; cls: string }> = {
  running: { label: "Executando", icon: <path d="M1 0l8 5-8 5z" />, cls: "border-primary text-neon" },
  waiting: { label: "Aguardando você", icon: <><rect x="4" y="0" width="2" height="6" /><rect x="4" y="8" width="2" height="2" /></>, cls: "border-neon bg-neon font-semibold text-bg" },
  done: { label: "Concluído", icon: <path d="M1 5.5l2.5 2.5L9 2.5" fill="none" stroke="currentColor" strokeWidth="1.8" />, cls: "border-line text-text" },
  ended: { label: "Encerrado", icon: <rect x="2" y="2" width="6" height="6" />, cls: "border-line text-muted" },
  idle: { label: "Ocioso", icon: <circle cx="5" cy="5" r="2" />, cls: "border-line text-muted" },
  error: { label: "Erro", icon: <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" fill="none" stroke="currentColor" strokeWidth="1.8" />, cls: "border-neon bg-primary font-semibold text-text" },
};

const DELEGATION: Record<DelegationStatus, { label: string; cls: string }> = {
  draft: { label: "Rascunho", cls: "border-line text-muted" },
  running: { label: "Executando", cls: "border-primary text-neon" },
  checkpoint: { label: "Aprova?", cls: "border-neon bg-neon font-semibold text-bg" },
  handoff: { label: "Passagem", cls: "border-primary text-text" },
  blocked: { label: "Bloqueada", cls: "border-neon bg-primary font-semibold text-text" },
  failed: { label: "Falhou", cls: "border-neon bg-primary font-semibold text-text" },
  done: { label: "Concluída", cls: "border-line text-text" },
  cancelled: { label: "Cancelada", cls: "border-line text-muted" },
};

const base = "inline-flex items-center gap-1.5 whitespace-nowrap border px-[7px] pt-[3px] pb-[2px] text-[10px] uppercase tracking-[0.12em]";

export function StatusBadge({ status, className = "" }: { status: SessionStatus; className?: string }) {
  const s = SESSION[status];
  return (
    <span className={`${base} ${s.cls} ${className}`}>
      <svg className="h-2.5 w-2.5 shrink-0 fill-current" viewBox="0 0 10 10" aria-hidden="true">{s.icon}</svg>
      {s.label}
    </span>
  );
}

export function DelegationBadge({ status, className = "" }: { status: DelegationStatus; className?: string }) {
  const s = DELEGATION[status];
  return <span className={`${base} ${s.cls} ${className}`}>{s.label}</span>;
}
