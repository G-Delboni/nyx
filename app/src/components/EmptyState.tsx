import type { ReactNode } from "react";

export interface EmptyStateProps {
  /** One short line in Nyx's voice (dry, no exclamation). */
  children: ReactNode;
  /** Optional small HUD label above the text, e.g. "DELEGAR". */
  title?: string;
  /** A single action (Button) under the text. */
  action?: ReactNode;
}

/** Dashed, centred empty block: an invitation to act, not a mood. */
export function EmptyState({ children, title, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center gap-2 border border-dashed border-line px-3 py-[18px] text-center text-muted">
      {title && <span className="text-[10px] uppercase tracking-[0.14em] text-primary">{title}</span>}
      <p className="m-0 max-w-[32ch] text-balance">{children}</p>
      {action && <div className="mt-1">{action}</div>}
    </div>
  );
}
