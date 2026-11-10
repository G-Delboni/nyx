export type AvatarState = "intro" | "idle" | "analyzing" | "alert";

export interface AvatarStats {
  fps: number;
  targetFps: number;
  frameMs: number;
  calls: number;
  triangles: number;
  heap: string;
}

export interface AvatarOptions {
  modelUrl: string;
  reducedMotion?: boolean;
  /** "on" = ASCII birth, background and alert glitch; "introOnly" = only the opening scan; "off" = no pass at all. */
  asciiMode?: "on" | "introOnly" | "off";
  /** Apply the stand-in look (recolour, headphones, bangs...) to the loaded model. Default: true only for sample.vrm. */
  standInTweaks?: boolean;
  onLoad?: () => void;
  onIntroEnd?: () => void;
  onProgress?: (fraction: number) => void;
  onError?: (e: unknown) => void;
  onStats?: (s: AvatarStats) => void;
}

export interface AvatarEngine {
  setState(s: AvatarState): void;
  getState(): AvatarState;
  /** Dissolve to ASCII (300 ms) and stop rendering. */
  hide(): void;
  /** Resume rendering; `withIntro` replays the opening. */
  show(withIntro?: boolean): void;
  skipIntro(): void;
  /** Dev: render one frame now, even while the document is hidden. */
  tick(): void;
  isLoaded(): boolean;
  /**
   * Drive the mouth from the TTS amplitude envelope (one value 0..1 per 50 ms, from `SpeechEvent.envelope`),
   * timed from `startedAtMs` (performance.now() when the audio started). `null` closes the mouth.
   */
  setSpeech(envelope: number[] | null, startedAtMs?: number): void;
  /** Smallest hand-vertex clearance above the key tops, in mm (NaN before the model loads). */
  checkFingerClearance(o?: { press?: number; lean?: number; rest?: number; typing?: number; dip?: number; t?: number }): number;
  /** Face x monitor overlaps over 3 aspects x 4 poses x 11 dolly steps (NaN before the model loads). */
  checkMonitorOverlap(): number;
  dispose(): void;
}

export function createAvatar(container: HTMLElement, opts: AvatarOptions): AvatarEngine;
