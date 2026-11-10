import { Component, lazy, Suspense, useEffect, useRef, type ReactNode } from "react";
import pkg from "../../package.json";
import type { Settings, Snapshot } from "../api/contract";
import { Microtext } from "../components/Microtext";
import { NyxLine } from "../components/NyxLine";
import { SpeechWave } from "../components/SpeechWave";
import type { CurrentSpeech } from "../hooks/useSpeech";
import { Portrait } from "./Portrait";
import { speechCaption, typingStepFor } from "./speech";

export const STATUS_TEXT: Record<Snapshot["status"], string> = {
  active: "STATUS: ACTIVE",
  analyzing: "ANALYZING...",
  alert: "STATUS: ALERT",
};

const CAM_STATE: Record<Snapshot["status"], string> = { active: "IDLE", analyzing: "ANALYZING", alert: "ALERT" };

const Avatar3d = lazy(() => import("./avatar3d/Avatar3d"));

/** Any failure in the 3D avatar (WebGL, model, shader) falls back to the portrait, silently. */
class AvatarBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch(err: unknown) { console.warn("[nyx] avatar3d failed, using portrait", err); }
  render() { return this.state.failed ? <Portrait /> : this.props.children; }
}

export interface NyxSceneProps {
  snap: Snapshot | null;
  /** Nyx's remark (from nyxLine()). Re-typed only when it changes. */
  line: string | null;
  sceneMode: Settings["sceneMode"];
  asciiMode?: Settings["asciiMode"];
  /** Name shown as CTX; null = GERAL. */
  contextName: string | null;
  /** What Nyx is saying now (useSpeech()): shown as her line while it plays, moves the avatar's mouth. */
  speech?: CurrentSpeech | null;
  /** Quick mute (settings.voiceMuted); omit `onToggleMute` to hide the control. */
  muted?: boolean;
  onToggleMute?: () => void;
}

/** 12 px speaker glyph; `off` draws the slash and drops the waves. */
function SpeakerIcon({ off }: { off: boolean }) {
  return (
    <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" strokeLinecap="round" aria-hidden="true">
      <path d="M1.5 4.5h2l2.5-2.2v7.4L3.5 7.5h-2z" />
      {off ? <path d="M8 4.5l3 3M11 4.5l-3 3" /> : <><path d="M8 4.6a2 2 0 0 1 0 2.8" /><path d="M9.4 3.2a4 4 0 0 1 0 5.6" /></>}
    </svg>
  );
}

function Counter({ n, label, hot }: { n: number; label: string; hot?: boolean }) {
  return (
    <div className={`counter cut ${hot ? "hot" : ""}`}>
      <div>
        <span className="n">{n}</span>
        <span className="l">{label}</span>
      </div>
    </div>
  );
}

/** Top of the panel: the portrait (or the beta 3D avatar), the written HUD, Nyx's line and the counters. */
export function NyxScene({ snap, line, sceneMode, asciiMode = "on", contextName, speech = null, muted = false, onToggleMute }: NyxSceneProps) {
  const status = snap?.status ?? "active";
  const running = snap?.agents.filter((a) => a.status === "running").length ?? 0;
  const waiting = snap?.agents.filter((a) => a.status === "waiting").length ?? 0;
  const delegations = snap?.activeDelegations ?? 0;

  // While she speaks, her line is what she says (typed in step with the audio, which starts with the event);
  // when the audio ends the usual remark comes back ready, without typing again.
  const caption = speech ? speechCaption(speech.speech.text) : null;
  const shown = caption ?? line;
  const wasSpeaking = useRef(false);
  const instant = !speech && wasSpeaking.current;
  useEffect(() => { wasSpeaking.current = !!speech; }, [speech]);
  const stepMs = speech && caption ? typingStepFor(speech, caption.length) : undefined;

  return (
    <header className="scene" aria-label="Cena da Nyx">
      {sceneMode === "avatar3d" ? (
        <AvatarBoundary>
          <Suspense fallback={<Portrait />}>
            <Avatar3d state={status} asciiMode={asciiMode} speech={speech} />
          </Suspense>
        </AvatarBoundary>
      ) : (
        <Portrait />
      )}

      <div className="scene-frame" aria-hidden="true"><i /><i /><i /><i /></div>
      <div className="scene-ruler" aria-hidden="true" />
      <div className="scene-ruler v" aria-hidden="true" />

      <div className="absolute top-[18px] left-5 flex flex-col gap-0.5">
        <Microtext className="tracking-[0.14em]">NYX // OPERADORA</Microtext>
        <Microtext tone="neon" cursor className="text-[11px] font-semibold tracking-[0.14em]" role="status" aria-live="polite">
          {snap ? STATUS_TEXT[status] : "CONNECTING..."}
        </Microtext>
        <Microtext className="tracking-[0.14em]">SCN 01 · CAM A · {snap ? CAM_STATE[status] : "---"}</Microtext>
      </div>
      <div className="absolute top-[18px] right-5 flex flex-col items-end gap-0.5">
        <Microtext className="max-w-[120px] truncate" aria-hidden="true">CTX {contextName ?? "GERAL"}</Microtext>
        <Microtext aria-hidden="true">v{pkg.version}</Microtext>
        {onToggleMute && (
          <button
            type="button"
            className="scene-mute"
            aria-pressed={muted}
            aria-label={muted ? "Ativar a voz da Nyx" : "Silenciar a voz da Nyx"}
            title={muted ? "Voz silenciada" : "Silenciar a voz"}
            onClick={onToggleMute}
          >
            {muted && <span aria-hidden="true">MUDO</span>}
            <SpeakerIcon off={muted} />
          </button>
        )}
      </div>

      {shown && <NyxLine text={shown} stepMs={stepMs} instant={instant} lead={<SpeechWave speech={speech} />} />}

      <div className="absolute bottom-3.5 left-5 flex gap-1.5">
        <Counter n={running} label="rodando" />
        <Counter n={waiting} label="aguardando você" hot={waiting > 0} />
        {delegations > 0 && <Counter n={delegations} label={delegations === 1 ? "delegação" : "delegações"} />}
      </div>
    </header>
  );
}
