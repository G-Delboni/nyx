import type { ReactNode } from "react";
import { Button } from "./Button";

export interface ConfirmCardProps {
  /** The question, stated plainly (what happens, what cannot be undone). */
  children: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
  busy?: boolean;
  /** danger = destructive (violet, never red). */
  tone?: "default" | "danger";
  label?: string;
}

/** Inline confirmation (role=alertdialog) that replaces the triggering buttons, not a modal. */
export function ConfirmCard({ children, confirmLabel, cancelLabel = "Voltar", onConfirm, onCancel, busy, tone = "default", label = "Confirmar" }: ConfirmCardProps) {
  return (
    <div role="alertdialog" aria-label={label} className="cut border border-neon bg-surface-2 p-2.5">
      <p className="m-0 text-[11px] text-text">{children}</p>
      <div className="mt-2 flex gap-1.5">
        <Button variant={tone === "danger" ? "danger" : "primary"} disabled={busy} onClick={onConfirm} autoFocus>{confirmLabel}</Button>
        <Button variant="ghost" onClick={onCancel}>{cancelLabel}</Button>
      </div>
    </div>
  );
}
