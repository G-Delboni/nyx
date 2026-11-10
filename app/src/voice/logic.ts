// Pure helpers for the Voz settings section and the usage line (pt-BR).
import type { Settings, VoiceSpeakOn, VoiceStatus } from "../api/contract";

export const TEST_PHRASE = "De volta, Dev. Voz configurada.";
export const DEFAULT_FREE_TIER = 500_000;

export const AZURE_VOICES: Array<{ id: string; label: string }> = [
  { id: "pt-BR-FranciscaNeural", label: "Francisca (padrão)" },
  { id: "pt-BR-ThalitaNeural", label: "Thalita" },
  { id: "pt-BR-BrendaNeural", label: "Brenda" },
  { id: "pt-BR-ElzaNeural", label: "Elza" },
  { id: "pt-BR-GiovannaNeural", label: "Giovanna" },
  { id: "pt-BR-LeilaNeural", label: "Leila" },
  { id: "pt-BR-LeticiaNeural", label: "Letícia" },
  { id: "pt-BR-ManuelaNeural", label: "Manuela" },
  { id: "pt-BR-YaraNeural", label: "Yara" },
];

export const SPEAK_ON: Array<{ key: keyof VoiceSpeakOn; label: string }> = [
  { key: "alerts", label: "Alertas" },
  { key: "opening", label: "Abertura do painel" },
  { key: "chat", label: "Respostas do chat" },
  { key: "longTask", label: "Tarefa longa e uso" },
];

export const ENGINES: Array<{ id: Settings["voiceEngine"]; label: string }> = [
  { id: "auto", label: "Automático" },
  { id: "azure", label: "Azure Neural" },
  { id: "windows", label: "Windows" },
];

export const clamp = (n: number, lo: number, hi: number): number =>
  Math.min(hi, Math.max(lo, Number.isFinite(n) ? n : lo));

/** Rate/pitch offset in percent, integer, -20..+20. */
export const clampOffset = (n: number): number => Math.round(clamp(n, -20, 20));

/** Volume 0..1, two decimals. */
export const clampVolume = (n: number): number => Math.round(clamp(n, 0, 1) * 100) / 100;

/** -5 -> "−5%", 0 -> "0%", 10 -> "+10%". */
export function formatOffset(n: number): string {
  const v = Math.round(n);
  if (v === 0) return "0%";
  return `${v > 0 ? "+" : "−"}${Math.abs(v)}%`;
}

export const formatVolume = (v: number): string => `${Math.round(clampVolume(v) * 100)}%`;

const nf0 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });

/** 12345 -> "12.345". */
export const formatChars = (n: number): string => nf0.format(Math.max(0, Math.round(n)));

/** Percent of the free tier used (0..100, may exceed 100 -> capped for the bar). */
export function freeTierPercent(used: number, limit: number = DEFAULT_FREE_TIER): number {
  if (!(limit > 0)) return 0;
  return clamp((Math.max(0, used) / limit) * 100, 0, 100);
}

export function engineLabel(e: VoiceStatus["engineInUse"]): string {
  return e === "azure" ? "Azure Neural" : e === "windows" ? "Windows" : "nenhum";
}

/** Short line about whether the voice is silenced right now, or null when it speaks. */
export function silenceReason(s: Pick<Settings, "voiceEnabled" | "voiceMuted" | "doNotDisturb">): string | null {
  if (!s.voiceEnabled) return "Voz desligada.";
  if (s.voiceMuted) return "Mudo: ela não fala até você desligar.";
  if (s.doNotDisturb) return "Não perturbe ligado: a voz fica em silêncio.";
  return null;
}

export const AZURE_KEY_HINT = "defina NYX_AZURE_SPEECH_KEY e NYX_AZURE_SPEECH_REGION e reinicie a Nyx";
