import type { ReactNode } from "react";
import type { DelegationStep } from "../api/contract";

export type StepState = DelegationStep["status"];

export interface StepperItem {
  title: string;
  state: StepState;
  /** Shown under the title (report lines, live activity...). */
  detail?: ReactNode;
}

const MARK: Record<StepState, { glyph: string; cls: string }> = {
  pending: { glyph: "·", cls: "border-line text-muted" },
  running: { glyph: "▶", cls: "border-primary text-neon" },
  checkpoint: { glyph: "?", cls: "border-neon bg-neon text-bg font-semibold" },
  approved: { glyph: "✓", cls: "border-primary text-text" },
  failed: { glyph: "✕", cls: "border-neon bg-primary text-text" },
};

const LABEL: Record<StepState, string> = {
  pending: "pendente", running: "executando", checkpoint: "aguardando aprovação", approved: "aprovada", failed: "falhou",
};

/** Vertical list of delegation steps: a numbered mark, a rail, the title and an optional detail. */
export function Stepper({ items, current }: { items: StepperItem[]; current?: number }) {
  return (
    <ol className="m-0 flex list-none flex-col p-0">
      {items.map((s, i) => {
        const m = MARK[s.state];
        const last = i === items.length - 1;
        const isCurrent = current === i;
        return (
          <li key={i} className="relative flex gap-2.5 pb-2.5" aria-current={isCurrent ? "step" : undefined}>
            {!last && <span aria-hidden="true" className={`absolute top-[18px] bottom-0 left-[8px] w-px ${s.state === "approved" ? "bg-primary" : "bg-line"}`} />}
            <span aria-hidden="true" className={`relative z-10 flex h-[17px] w-[17px] shrink-0 items-center justify-center border text-[10px] leading-none ${m.cls}`}>{m.glyph}</span>
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline justify-between gap-2">
                <span className={`font-display text-sm font-semibold leading-tight ${s.state === "pending" ? "text-muted" : "text-text"}`}>
                  <span className="sr-only">Passo {i + 1}, {LABEL[s.state]}: </span>
                  {s.title}
                </span>
                <span className="shrink-0 text-[10px] uppercase tracking-[0.12em] text-muted">{i + 1}/{items.length}</span>
              </div>
              {s.detail && <div className="mt-1 text-[11px] text-muted">{s.detail}</div>}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
