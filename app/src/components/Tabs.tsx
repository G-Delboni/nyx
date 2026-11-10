import { useRef, type KeyboardEvent } from "react";

export interface TabItem<Id extends string> {
  id: Id;
  label: string;
  /** Small neon count next to the label (0/undefined hides it). */
  badge?: number;
}

export interface TabsProps<Id extends string> {
  items: readonly TabItem<Id>[];
  value: Id;
  onChange: (id: Id) => void;
  /** id of the tabpanel the tabs control. */
  panelId: string;
  label?: string;
}

/** Tab strip (role=tablist) with arrow/Home/End keyboard navigation. Content-sized, spread across the width. */
export function Tabs<Id extends string>({ items, value, onChange, panelId, label = "Seções" }: TabsProps<Id>) {
  const refs = useRef<Partial<Record<Id, HTMLButtonElement | null>>>({});

  function onKeyDown(e: KeyboardEvent) {
    const i = items.findIndex((t) => t.id === value);
    let n = i;
    if (e.key === "ArrowRight") n = (i + 1) % items.length;
    else if (e.key === "ArrowLeft") n = (i - 1 + items.length) % items.length;
    else if (e.key === "Home") n = 0;
    else if (e.key === "End") n = items.length - 1;
    else return;
    e.preventDefault();
    onChange(items[n].id);
    refs.current[items[n].id]?.focus();
  }

  return (
    <nav role="tablist" aria-label={label} className="tabs relative flex shrink-0 justify-between border-b border-line bg-surface px-1.5" onKeyDown={onKeyDown}>
      {items.map((t) => {
        const sel = t.id === value;
        return (
          <button
            key={t.id}
            ref={(el) => { refs.current[t.id] = el; }}
            role="tab"
            id={`tab-${t.id}`}
            aria-selected={sel}
            aria-controls={panelId}
            tabIndex={sel ? 0 : -1}
            onClick={() => onChange(t.id)}
          >
            {t.label}
            {t.badge ? <sup aria-label={`${t.badge} não lidos`}>{t.badge}</sup> : null}
          </button>
        );
      })}
    </nav>
  );
}
