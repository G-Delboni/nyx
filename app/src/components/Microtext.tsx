import type { HTMLAttributes, ReactNode } from "react";

export interface MicrotextProps extends HTMLAttributes<HTMLSpanElement> {
  children: ReactNode;
  /** neon for the live value, muted (default) for labels. */
  tone?: "muted" | "neon" | "text";
  /** Show the ▮ cursor after the text (HUD status line). */
  cursor?: boolean;
}

/** HUD microtext: 10px, uppercase, tracked. For `STATUS: ACTIVE`, `SCN 01 · CAM A`, labels. */
export function Microtext({ children, tone = "muted", cursor = false, className = "", ...rest }: MicrotextProps) {
  const color = tone === "neon" ? "text-neon" : tone === "text" ? "text-text" : "text-muted";
  return (
    <span className={`text-[10px] uppercase tracking-[0.12em] ${color} ${className}`} {...rest}>
      {children}
      {cursor && <span className="cursor" aria-hidden="true" />}
    </span>
  );
}
