// The short line Nyx "says" in the corner of the scene. Pure: same inputs, same output.
// Tone (PERSONAGEM §2): dry, short, no exclamation, no emoji; the user is always "Dev"; direct address ("Dev, ...") only for error,
// block and usage alert; "Dev" otherwise. Pass a stable `now` (e.g. the moment the panel
// opened), because the opening prefix and the rare joke are seeded from it.
import type { AgentSession, Delegation, Snapshot } from "../api/contract";

export interface NyxLineOpts {
  justOpened: boolean;
  now: Date;
}

const MAX_NAME = 22;
const QUIET = "Tudo quieto.";
const JOKES = [
  "Nenhum agente pegou fogo hoje.",
  "Nada a reportar, estranhamente.",
  "Silêncio total, o que é suspeito.",
];

/** 32-bit integer hash (deterministic seed from a timestamp). */
function hash(n: number): number {
  let x = (Math.floor(n) ^ 0x9e3779b9) >>> 0;
  x = Math.imul(x ^ (x >>> 16), 0x85ebca6b) >>> 0;
  x = Math.imul(x ^ (x >>> 13), 0xc2b2ae35) >>> 0;
  return (x ^ (x >>> 16)) >>> 0;
}

/** True for about 1 in 10 seeds: this opening may carry the dry joke. */
export function isJokeSeed(now: Date): boolean {
  return hash(now.getTime()) % 10 === 0;
}

function short(name: string): string {
  const t = name.trim();
  return t.length > MAX_NAME ? `${t.slice(0, MAX_NAME - 1)}…` : t;
}

function cap(s: string): string {
  return s ? s[0].toUpperCase() + s.slice(1) : s;
}

function folderOf(a: AgentSession): string {
  const name = a.folderName ?? a.cwd?.split(/[\\/]/).filter(Boolean).pop() ?? "";
  return name ? cap(short(name)) : "a sessão";
}

const newest = <T extends { updatedAt: string }>(list: T[]): T =>
  list.reduce((a, b) => (Date.parse(b.updatedAt) > Date.parse(a.updatedAt) ? b : a));

interface Item {
  /** Text without the opening prefix. For checkpoint, `withPrefix` joins with a comma. */
  text: string;
  /** Serious items skip the friendly opening prefix and address the user directly ("Dev, ..."). */
  serious: boolean;
  joinWithComma?: boolean;
}

function urgentItem(snap: Snapshot, delegations: Delegation[]): Item {
  const ctxName = (id: string) => snap.contexts.find((c) => c.id === id)?.name;

  const checkpoints = delegations.filter((d) => d.status === "checkpoint");
  if (checkpoints.length) {
    const d = newest(checkpoints);
    const n = Math.min(d.currentStep + 1, d.steps.length);
    return { text: `Etapa ${n} de ${d.steps.length} concluída. Aprova?`, serious: false, joinWithComma: true };
  }

  const blocked = delegations.filter((d) => d.status === "blocked");
  if (blocked.length) {
    const name = ctxName(newest(blocked).contextId);
    return { text: `Dev, ${name ? cap(short(name)) : "a delegação"} travou numa permissão.`, serious: true };
  }

  const waiting = snap.agents.filter((a) => a.status === "waiting");
  if (waiting.length) {
    return { text: `${folderOf(newest(waiting))} quer uma resposta sua.`, serious: false };
  }

  const errored = snap.agents.filter((a) => a.status === "error");
  if (errored.length) {
    return { text: `Dev, falhou em ${folderOf(newest(errored))}.`, serious: true };
  }

  const pct = snap.weekUsage.percent;
  if (pct != null && pct >= 95) return { text: `Dev, semana em ${Math.round(pct)}% do teto.`, serious: true };
  if (pct != null && pct >= 80) return { text: `Semana em ${Math.round(pct)}% do teto.`, serious: false };

  const running = snap.agents.filter((a) => a.status === "running").length;
  if (running > 0) return { text: running === 1 ? "1 agente rodando." : `${running} agentes rodando.`, serious: false };

  return { text: QUIET, serious: false };
}

export function nyxLine(snapshot: Snapshot, delegations: Delegation[], opts: { justOpened: boolean; now: Date }): string {
  const seed = hash(opts.now.getTime());
  // The joke only replaces "Tudo quieto." on an opening, so it can never meet an error or alert.
  const joke = opts.justOpened && seed % 10 === 0;
  const item = urgentItem(snapshot, delegations);
  if (joke && item.text === QUIET) item.text = JOKES[(seed >>> 8) % JOKES.length];

  if (!opts.justOpened) return item.text;

  const opening = item.serious ? "De volta." : (seed >>> 4) % 4 === 0 ? "Online, Dev." : "De volta, Dev.";
  if (item.joinWithComma) return `${opening} ${item.text.replace(". Aprova?", ", aprova?")}`;
  return `${opening} ${item.text}`;
}
