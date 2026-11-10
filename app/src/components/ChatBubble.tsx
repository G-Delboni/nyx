import type { ReactNode } from "react";

export interface ChatBubbleProps {
  role: "user" | "nyx";
  children: ReactNode;
  /** Time or cost, muted, under the text. */
  meta?: ReactNode;
  /** Action cards (ConfirmCard / Buttons) proposed by Nyx. */
  actions?: ReactNode;
}

/** One message: the user on the right (surface-2), Nyx on the left with a neon rail. */
export function ChatBubble({ role, children, meta, actions }: ChatBubbleProps) {
  return (
    <div className={`flex flex-col gap-1 ${role === "user" ? "items-end" : "items-start"}`}>
      <div className={`bubble ${role}`}>
        <span className="sr-only">{role === "user" ? "Você: " : "Nyx: "}</span>
        <div className="whitespace-pre-wrap break-words">{children}</div>
      </div>
      {actions && <div className="flex max-w-[88%] flex-col gap-1.5">{actions}</div>}
      {meta && <span className="text-[10px] tracking-[0.06em] text-muted">{meta}</span>}
    </div>
  );
}
