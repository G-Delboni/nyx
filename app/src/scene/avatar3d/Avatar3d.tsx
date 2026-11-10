import { useEffect, useRef, useState } from "react";
import type { Settings, Snapshot } from "../../api/contract";
import type { CurrentSpeech } from "../../hooks/useSpeech";
import { Portrait } from "../Portrait";
import type { AvatarEngine, AvatarState } from "./engine";

export interface Avatar3dProps {
  state: Snapshot["status"];
  asciiMode: Settings["asciiMode"];
  /** What Nyx is saying now (from useSpeech()); its envelope moves her mouth. */
  speech?: CurrentSpeech | null;
}

/**
 * Drop-in model (docs/AVATAR-3D.md §1): put Nyx's real VRoid export at `app/public/models/nyx.vrm`
 * and it is used; until then the three-vrm sample stands in. No code change needed to swap.
 */
const MODEL_CANDIDATES = ["/models/nyx.vrm", "/models/sample.vrm"];

async function resolveModelUrl(): Promise<string> {
  for (const url of MODEL_CANDIDATES) {
    try {
      const r = await fetch(url, { method: "HEAD" });
      // Vite's dev server answers unknown paths with index.html (200): only accept a non-HTML body.
      if (r.ok && !(r.headers.get("content-type") ?? "").includes("text/html")) return url;
    } catch { /* try the next one */ }
  }
  return MODEL_CANDIDATES[MODEL_CANDIDATES.length - 1];
}
/** After this long hidden, the scene and its WebGL context are released (AVATAR-3D.md §5). */
const UNLOAD_AFTER_MS = 60_000;

const STATE: Record<Snapshot["status"], AvatarState> = { active: "idle", analyzing: "analyzing", alert: "alert" };

/**
 * Beta 3D avatar (three.js + three-vrm + ASCII pass), loaded only while the panel shows it.
 * Hidden window: the engine stops its loop at once; after 60 s it is disposed and comes back with the
 * intro when the window shows again. Any failure (no WebGL, no model) falls back to the portrait.
 */
export default function Avatar3d({ state, asciiMode, speech = null }: Avatar3dProps) {
  const host = useRef<HTMLDivElement>(null);
  const engine = useRef<AvatarEngine | null>(null);
  const wanted = useRef<AvatarState>(STATE[state]);
  const [failed, setFailed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [gen, setGen] = useState(0); // bump to rebuild the engine after an unload

  wanted.current = STATE[state];

  useEffect(() => {
    const el = host.current;
    if (!el || failed) return;
    let eng: AvatarEngine | null = null;
    let unloadTimer = 0;
    let cancelled = false;
    setLoaded(false);
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    void Promise.all([import("./engine.js"), resolveModelUrl()]).then(([{ createAvatar }, modelUrl]) => {
      if (cancelled) return;
      try {
        eng = createAvatar(el, {
          modelUrl,
          reducedMotion,
          asciiMode,
          onLoad: () => { setLoaded(true); },
          onIntroEnd: () => eng?.setState(wanted.current),
          onError: () => setFailed(true),
        });
        engine.current = eng;
        if (import.meta.env.DEV) (window as unknown as { __nyxAvatar?: AvatarEngine }).__nyxAvatar = eng;
      } catch (e) {
        console.warn("[nyx] avatar3d: WebGL unavailable, using portrait", e);
        setFailed(true);
      }
    }).catch((e) => { console.warn("[nyx] avatar3d: failed to load", e); setFailed(true); });

    const onVisibility = () => {
      if (document.hidden) {
        unloadTimer = window.setTimeout(() => { eng?.dispose(); eng = null; engine.current = null; setGen((g) => g + 1); }, UNLOAD_AFTER_MS);
      } else {
        clearTimeout(unloadTimer);
      }
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      cancelled = true;
      clearTimeout(unloadTimer);
      document.removeEventListener("visibilitychange", onVisibility);
      eng?.dispose();
      engine.current = null;
    };
  }, [failed, gen, asciiMode]); // a new ASCII mode rebuilds the engine (rare: only when the setting changes)

  // Snapshot status drives the state, except during the intro (applied when it ends).
  useEffect(() => {
    const eng = engine.current;
    if (eng && eng.isLoaded() && eng.getState() !== "intro") eng.setState(STATE[state]);
  }, [state, loaded]);

  // The TTS envelope reaches the engine as soon as it exists (a speech that started before the model loaded
  // is applied on load, timed from its real start, so the mouth lands mid-sentence instead of restarting).
  useEffect(() => {
    const eng = engine.current;
    if (eng && eng.isLoaded()) eng.setSpeech(speech ? speech.speech.envelope : null, speech?.startedAt);
  }, [speech, loaded]);

  if (failed) return <Portrait />;
  return (
    <>
      <div ref={host} className="absolute inset-0 [&>canvas]:block [&>canvas]:h-full [&>canvas]:w-full" aria-hidden="true" />
      {/* Same penumbra as the portrait (direction E): radial vignette centred on her, bottom fade under the counters. */}
      {loaded && <div className="scene-vignette fluid" aria-hidden="true" />}
      {!loaded && <Portrait />}
    </>
  );
}
