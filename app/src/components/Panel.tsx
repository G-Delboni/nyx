import type { ReactNode } from "react";

/** The panel shell: 420px max, full height, column layout, violet pool behind the content. */
export function Panel({ children }: { children: ReactNode }) {
  return (
    <div className="panel-pool relative mx-auto flex h-full max-w-[420px] flex-col overflow-hidden bg-bg">
      <h1 className="sr-only">Nyx</h1>
      {children}
    </div>
  );
}

/** Scrollable content area of the panel (one per tab). */
export function PanelMain({ children, ...rest }: { children: ReactNode } & React.HTMLAttributes<HTMLElement>) {
  return (
    <main {...rest} className={`relative min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain px-3 pt-2.5 pb-3 ${rest.className ?? ""}`}>
      {children}
    </main>
  );
}
