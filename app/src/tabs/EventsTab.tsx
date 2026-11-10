import { useCallback, useEffect, useState } from "react";
import { api } from "../api/client";
import type { EventKind, NyxEvent } from "../api/contract";
import { Button, EmptyState } from "../components";
import { relativeTime } from "../lib/format";

const ICON: Record<EventKind, string> = {
  taskDone: "✓", needsInput: "!", error: "✕", sessionStart: "▶", sessionEnd: "■",
  promptSubmitted: "›", toolUse: "·", usageThreshold: "%",
};

export function EventsTab({ now, eventTick, onChanged }: { now: number; eventTick: number; onChanged: () => void }) {
  const [events, setEvents] = useState<NyxEvent[] | null>(null);

  const load = useCallback(async () => {
    setEvents(await api.getEvents(100));
  }, []);

  useEffect(() => { void load(); }, [load, eventTick]);

  async function markRead(ids: number[]) {
    await Promise.all(ids.map((id) => api.markEventRead(id)));
    await load();
    onChanged();
  }

  if (!events) return <p className="m-0 text-muted">Carregando…</p>;
  const unread = events.filter((e) => !e.read);
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[10px] uppercase tracking-[0.12em] text-muted">
          {unread.length === 0 ? "tudo lido" : `${unread.length} não ${unread.length === 1 ? "lido" : "lidos"}`}
        </span>
        <Button variant="ghost" size="sm" disabled={unread.length === 0} onClick={() => markRead(unread.map((e) => e.id))}>
          Marcar todos como lidos
        </Button>
      </div>
      {events.length === 0 && <EmptyState title="Eventos">Nada por enquanto. Silêncio é bom sinal.</EmptyState>}
      <ul className="m-0 flex list-none flex-col gap-1 p-0">
        {events.map((e) => (
          <li key={e.id} className={`evt ${e.read ? "" : "unread"}`}>
            <span aria-hidden="true" className={`w-3 shrink-0 text-center font-semibold ${e.read ? "" : "text-neon"}`}>{ICON[e.kind]}</span>
            <div className="min-w-0 flex-1">
              <div className={e.read ? "" : "font-medium"}>
                {!e.read && <span className="sr-only">Não lido: </span>}
                {e.title}
              </div>
              {e.detail && <div className="mt-0.5 text-[11px] text-muted">{e.detail}</div>}
              <div className="mt-0.5 text-[10px] tracking-[0.06em] text-muted">
                {relativeTime(e.createdAt, now)}{e.priority === "high" ? " · prioridade alta" : ""}
              </div>
            </div>
            {!e.read && (
              <Button variant="ghost" size="sm" onClick={() => markRead([e.id])} aria-label={`Marcar como lido: ${e.title}`}>lido</Button>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
