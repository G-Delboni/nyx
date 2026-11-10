// Pure formatting helpers (pt-BR). No Date.now() inside: callers pass `now`.
import type { TokenCounts } from "../api/contract";

const nf1 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 1 });
const nf0 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });
const usd = new Intl.NumberFormat("pt-BR", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** 950 -> "950", 12_345 -> "12,3k", 4_200_000 -> "4,2M". */
export function formatTokens(n: number): string {
  const v = Math.max(0, Math.round(n));
  if (v < 1000) return nf0.format(v);
  if (v < 1_000_000) return `${nf1.format(v / 1000)}k`;
  if (v < 1_000_000_000) return `${nf1.format(v / 1_000_000)}M`;
  return `${nf1.format(v / 1_000_000_000)}B`;
}

export function totalTokens(t: TokenCounts): number {
  return t.input + t.output + t.cacheWrite + t.cacheRead;
}

/** 12.5 -> "US$ 12,50". */
export function formatUsd(n: number): string {
  return `US$ ${usd.format(n)}`;
}

export function formatPercent(p: number | null | undefined): string {
  if (p == null || !Number.isFinite(p)) return "--";
  return `${nf0.format(p)}%`;
}

/** "agora", "há 5 min", "há 2 h", "há 3 d". Future or invalid dates read as "agora". */
export function relativeTime(iso: string, now: number = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "--";
  const s = Math.floor((now - t) / 1000);
  if (s < 45) return "agora";
  const m = Math.floor(s / 60);
  if (m < 60) return `há ${Math.max(1, m)} min`;
  const h = Math.floor(m / 60);
  if (h < 24) return `há ${h} h`;
  return `há ${Math.floor(h / 24)} d`;
}

/** "claude-opus-4-5-20251101" -> "Opus 4.5"; unknown models are returned as-is. */
export function shortModel(model: string): string {
  const m = /claude-(opus|sonnet|haiku)-(\d+)(?:-(\d{1,2}))?(?:-\d{8})?$/.exec(model);
  if (!m) return model;
  const fam = m[1][0].toUpperCase() + m[1].slice(1);
  return m[3] ? `${fam} ${m[2]}.${m[3]}` : `${fam} ${m[2]}`;
}

/** Number of filled segments out of `total` for a 0..100+ percentage. */
export function segments(percent: number | null | undefined, total = 20): number {
  if (percent == null || !Number.isFinite(percent)) return 0;
  return Math.max(0, Math.min(total, Math.round((percent / 100) * total)));
}

/** Like formatUsd, but keeps 4 decimals below one cent (API cost of a chat reply). */
export function formatUsdFine(n: number): string {
  if (n > 0 && n < 0.01) return `US$ ${n.toLocaleString("pt-BR", { minimumFractionDigits: 4, maximumFractionDigits: 4 })}`;
  return formatUsd(n);
}
