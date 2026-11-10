import { useEffect, useMemo, useRef, useState } from "react";
import { api, type OpenTab } from "./api/client";
import { Panel, PanelMain, SegmentedBar, Tabs } from "./components";
import { useDelegations } from "./hooks/useDelegations";
import { useOpenTab } from "./hooks/useOpenTab";
import { useOpeningSpeech } from "./hooks/useOpeningSpeech";
import { useSettingsLive } from "./hooks/useSettingsLive";
import { useSpeech } from "./hooks/useSpeech";
import { formatPercent, formatUsd } from "./lib/format";
import { nyxLine } from "./nyx/line";
import { NyxScene } from "./scene/NyxScene";
import { useSnapshot } from "./state";
import { AgentsTab } from "./tabs/AgentsTab";
import { ChatTab } from "./tabs/ChatTab";
import { ContextsTab } from "./tabs/ContextsTab";
import { DelegateTab } from "./tabs/DelegateTab";
import { EventsTab } from "./tabs/EventsTab";
import { SettingsTab } from "./tabs/SettingsTab";
import { UsageTab } from "./tabs/UsageTab";

const CEILING = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });
// Dev only: ?scene=avatar3d|portrait previews a scene mode without saving it.
const SCENE_OVERRIDE = import.meta.env.DEV
  ? ((["portrait", "avatar3d"] as const).find((m) => m === new URLSearchParams(location.search).get("scene")) ?? null)
  : null;
// Dev only: ?ascii=on|introOnly|off previews the ASCII mode of the 3D avatar.
const ASCII_OVERRIDE = import.meta.env.DEV
  ? ((["on", "introOnly", "off"] as const).find((m) => m === new URLSearchParams(location.search).get("ascii")) ?? null)
  : null;

const TAB_IDS = ["agents", "delegate", "usage", "events", "contexts", "chat", "settings"] as const;
type TabId = (typeof TAB_IDS)[number];
const LABEL: Record<TabId, string> = {
  agents: "Agentes", delegate: "Delegar", usage: "Uso", events: "Eventos", contexts: "Contextos", chat: "Nyx", settings: "Config",
};

export default function App() {
  const { snap, error, refresh, eventTick } = useSnapshot();
  const { delegations } = useDelegations(null);
  const [tab, setTab] = useState<TabId>("agents");
  const [now, setNow] = useState(() => Date.now());
  // The moment the panel opened: seeds the opening line (and the rare joke) once.
  const [openedAt] = useState(() => new Date());
  useOpeningSpeech(snap, delegations, openedAt);
  // Settings kept live through nyx://settings (the tray "Mudo" shows in the scene at once).
  const settings = useSettingsLive(snap?.settings);

  useOpenTab((t: OpenTab) => setTab(t));
  const speech = useSpeech();
  const muted = settings?.voiceMuted ?? false;
  const toggleMute = async () => { try { await api.setVoiceMuted(!muted); } finally { await refresh(); } };

  // Relative times refresh once a minute, and only while the window is visible.
  useEffect(() => {
    const tick = () => { if (!document.hidden) setNow(Date.now()); };
    const id = setInterval(tick, 60_000);
    document.addEventListener("visibilitychange", tick);
    return () => { clearInterval(id); document.removeEventListener("visibilitychange", tick); };
  }, []);

  // Nyx's line keeps the opening prefix while the situation is the one she opened with; after
  // that, updates read as plain remarks. Only text changes re-trigger the typing.
  const firstPlain = useRef<string | null>(null);
  const line = useMemo(() => {
    if (!snap) return null;
    const plain = nyxLine(snap, delegations, { justOpened: false, now: openedAt });
    firstPlain.current ??= plain;
    return plain === firstPlain.current ? nyxLine(snap, delegations, { justOpened: true, now: openedAt }) : plain;
  }, [snap, delegations, openedAt]);

  const contextName = useMemo(() => {
    if (!snap) return null;
    const live = snap.agents
      .filter((a) => a.contextId && a.status !== "ended")
      .sort((a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt))[0];
    return live ? (snap.contexts.find((c) => c.id === live.contextId)?.name ?? null) : null;
  }, [snap]);

  const week = snap?.weekUsage;
  const tabs = TAB_IDS.map((id) => ({ id, label: LABEL[id], badge: id === "events" ? snap?.unreadEvents : undefined }));

  return (
    <Panel>
      <NyxScene snap={snap} line={line} sceneMode={SCENE_OVERRIDE ?? snap?.settings.sceneMode ?? "portrait"} asciiMode={ASCII_OVERRIDE ?? snap?.settings.asciiMode ?? "on"} contextName={contextName}
        speech={speech} muted={muted} onToggleMute={snap ? () => void toggleMute() : undefined} />

      <section className="relative shrink-0 border-b border-line bg-bg px-3 pt-2 pb-[7px]" aria-label="Uso semanal">
        <div className="mb-[5px] flex justify-between text-[10px] uppercase tracking-[0.1em] text-muted">
          <span>
            Semana · <b className="font-medium text-text">{week ? formatUsd(week.costUsd) : "--"}</b>
            {week?.ceilingUsd != null && <> de {CEILING.format(week.ceilingUsd)} · est.</>}
          </span>
          <span><em className="not-italic text-neon">{formatPercent(week?.percent)}</em> do teto</span>
        </div>
        <SegmentedBar percent={week?.percent} label="Uso semanal contra o teto" marks={snap?.settings.alertThresholds ?? [80, 95]} />
      </section>

      <Tabs items={tabs} value={tab} onChange={setTab} panelId="panel" />

      <PanelMain id="panel" role="tabpanel" aria-labelledby={`tab-${tab}`}>
        {error && <p className="mb-2 border border-neon p-2 text-neon" role="alert">Sem resposta do núcleo: {error}</p>}
        {!snap ? (
          <p className="text-muted">Conectando…</p>
        ) : tab === "agents" ? (
          <AgentsTab snap={snap} now={now} onGoConfig={() => setTab("settings")} />
        ) : tab === "delegate" ? (
          <DelegateTab snap={snap} now={now} />
        ) : tab === "usage" ? (
          <UsageTab snap={snap} />
        ) : tab === "events" ? (
          <EventsTab now={now} eventTick={eventTick} onChanged={refresh} />
        ) : tab === "contexts" ? (
          <ContextsTab contexts={snap.contexts} onChanged={refresh} />
        ) : tab === "chat" ? (
          <ChatTab snap={snap} now={now} />
        ) : (
          <SettingsTab settings={settings ?? snap.settings} onSaved={refresh} />
        )}
      </PanelMain>
    </Panel>
  );
}
