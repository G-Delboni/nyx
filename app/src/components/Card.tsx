import type { HTMLAttributes, ReactNode } from "react";

export type CardTone = "default" | "running" | "waiting" | "error" | "idle" | "neon" | "primary";

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  /** Border colour graduates with the tone: line → primary (running) → neon (waiting). */
  tone?: CardTone;
  /** Cut corners (HUD). Default true. */
  cut?: boolean;
  children: ReactNode;
  bodyClassName?: string;
}

/** 1px-frame card: outer colour is the border, inner block is the surface. */
export function Card({ tone = "default", cut = true, className = "", bodyClassName = "", children, ...rest }: CardProps) {
  return (
    <div className={`card ${tone !== "default" ? `tone-${tone}` : ""} ${cut ? "cut" : ""} ${className}`} {...rest}>
      <div className={`card-body ${bodyClassName}`}>{children}</div>
    </div>
  );
}

/** Section heading inside a card: Rajdhani, neon, with an optional right-side meta. */
export function CardTitle({ children, meta }: { children: ReactNode; meta?: ReactNode }) {
  return (
    <h2 className="m-0 mb-1.5 flex items-baseline justify-between gap-2 font-display text-xs font-semibold uppercase tracking-[0.14em] text-neon">
      <span>{children}</span>
      {meta !== undefined && <span className="font-mono text-[10px] font-medium tracking-[0.04em] normal-case text-muted">{meta}</span>}
    </h2>
  );
}
