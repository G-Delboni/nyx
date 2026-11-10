import { useEffect, useState } from "react";
import { isTauri } from "../api/client";
import { api } from "../api/client";
import type { Snapshot, UsageRange, UsageSummary } from "../api/contract";
import { Button, Card, CardTitle, SegmentedBar } from "../components";
import { useVoiceStatus } from "../hooks/useVoiceStatus";
import { formatChars, freeTierPercent } from "../voice/logic";
import { formatPercent, formatTokens, formatUsd, shortModel } from "../lib/format";

const RANGES: Array<{ id: UsageRange; label: string; short: string }> = [
  { id: "week", label: "Semana (7 dias)", short: "semana" },
  { id: "today", label: "Hoje", short: "hoje" },
  { id: "session", label: "Sessão atual", short: "sessão" },
];

const tokensOf = (t: UsageSummary["tokens"]) => t.input + t.output + t.cacheRead + t.cacheWrite;

export function UsageTab({ snap }: { snap: Snapshot }) {
  const [data, setData] = useState<Partial<Record<UsageRange, UsageSummary>>>({});
  const [exported, setExported] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const { status: voice } = useVoiceStatus();
  const weekKey = snap.weekUsage.costUsd + ":" + snap.weekUsage.messages;

  useEffect(() => {
    let alive = true;
    void Promise.all(RANGES.map((r) => api.getUsage(r.id))).then((res) => {
      if (alive) setData(Object.fromEntries(res.map((u) => [u.range, u])));
    }).catch((e) => alive && setErr(String(e)));
    return () => { alive = false; };
  }, [weekKey]);

  const week = snap.weekUsage;
  const ceiling = snap.settings.weeklyCeilingUsd;
  const maxModel = Math.max(1, ...week.byModel.map((m) => tokensOf(m.tokens)));

  async function reveal(path: string) {
    try {
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      await revealItemInDir(path);
    } catch (e) { setErr(String(e)); }
  }

  async function exportCsv(range: UsageRange) {
    try {
      setExported(await api.exportUsageCsv(range));
      setErr(null);
    } catch (e) {
      setErr(String(e));
    }
  }

  return (
    <div className="flex flex-col gap-2">
      {RANGES.map((r) => {
        const u = r.id === "week" ? week : data[r.id];
        const isWeek = r.id === "week";
        return (
          <Card key={r.id}>
            <CardTitle meta={u ? `${u.messages} msgs · ${u.sessions} ${u.sessions === 1 ? "sessão" : "sessões"}` : "--"}>{r.label}</CardTitle>
            <div className="flex flex-wrap items-baseline gap-x-1.5">
              <span className="font-display text-[22px] leading-none font-bold text-text">{u ? formatUsd(u.costUsd) : "--"}</span>
              {isWeek && (
                <span className="text-[10px] tracking-[0.06em] text-muted">
                  teto {formatUsd(ceiling)} <span className="ml-1 border border-line px-1 uppercase tracking-[0.1em]">estimativa</span>
                </span>
              )}
            </div>
            {isWeek && (
              <div className="mt-2">
                <SegmentedBar percent={week.percent} label="Uso semanal contra o teto" marks={snap.settings.alertThresholds} />
                <div className="mt-1 text-right text-[10px] tracking-[0.1em] text-neon">{formatPercent(week.percent)} do teto</div>
              </div>
            )}
            {u && (
              <div className="mt-1.5 flex gap-2.5 text-[10px] tracking-[0.04em] text-muted">
                <span>in <b className="font-medium text-text">{formatTokens(u.tokens.input)}</b></span>
                <span>out <b className="font-medium text-text">{formatTokens(u.tokens.output)}</b></span>
                <span>cache <b className="font-medium text-text">{formatTokens(u.tokens.cacheRead + u.tokens.cacheWrite)}</b></span>
              </div>
            )}
          </Card>
        );
      })}

      <Card>
        <CardTitle meta="semana">Por modelo</CardTitle>
        {week.byModel.length === 0 ? (
          <p className="m-0 text-muted">Sem uso registrado.</p>
        ) : (
          <table className="tbl">
            <thead>
              <tr><th>modelo</th><th className="r">tokens</th><th className="r">custo est.</th></tr>
            </thead>
            <tbody>
              {week.byModel.map((m) => (
                <tr key={m.model}>
                  <td title={m.model}>
                    <span className="bar" style={{ width: `${Math.max(4, Math.round((tokensOf(m.tokens) / maxModel) * 70))}px` }} aria-hidden="true" />
                    {shortModel(m.model)}
                  </td>
                  <td className="r">{formatTokens(tokensOf(m.tokens))}</td>
                  <td className="r">{formatUsd(m.costUsd)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      {voice && (
        <Card>
          <CardTitle meta="mês">Nyx (voz)</CardTitle>
          <div className="flex flex-wrap items-baseline gap-x-1.5">
            <span className="font-display text-[22px] leading-none font-bold text-text">{formatChars(voice.monthChars)}</span>
            <span className="text-[10px] tracking-[0.06em] text-muted">
              de {formatChars(voice.freeTierChars)} caracteres <span className="ml-1 border border-line px-1 uppercase tracking-[0.1em]">estimativa</span>
            </span>
          </div>
          <div className="mt-2">
            <SegmentedBar percent={freeTierPercent(voice.monthChars, voice.freeTierChars)} label="Caracteres de voz contra o plano grátis do Azure" />
            <div className="mt-1 text-right text-[10px] tracking-[0.1em] text-neon">{formatPercent(freeTierPercent(voice.monthChars, voice.freeTierChars))} do plano grátis</div>
          </div>
          <p className="m-0 mt-1 text-[10px] text-muted">
            {voice.azureKeyPresent ? "Só conta a voz Azure; cache e voz do Windows não contam." : "Sem chave Azure: a voz do Windows não consome cota."}
          </p>
        </Card>
      )}

      <div className="flex flex-wrap items-center gap-1.5">
        {RANGES.map((r) => (
          <Button key={r.id} onClick={() => exportCsv(r.id)}>CSV · {r.short}</Button>
        ))}
      </div>
      {exported && (
        <div className="flex flex-col items-start gap-1.5">
          <p className="m-0 break-all text-[11px] text-neon" role="status">Exportado: {exported}</p>
          {isTauri() && <Button variant="ghost" onClick={() => reveal(exported)}>Mostrar na pasta</Button>}
        </div>
      )}
      {err && <p className="m-0 text-[11px] text-neon" role="alert">Falhou: {err}</p>}
    </div>
  );
}
