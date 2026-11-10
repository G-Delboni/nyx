import { useCallback, useEffect, useState } from "react";
import { api } from "../api/client";
import type { HooksReport, HooksStatus } from "../api/contract";
import { Button } from "./Button";
import { Card, CardTitle } from "./Card";
import { ConfirmCard } from "./ConfirmCard";

export function hooksSummary(s: HooksStatus): string {
  if (!s.hookExeExists) return "exe não encontrado";
  if (s.installed) return "instalado";
  const n = s.missingEvents.length;
  return n > 0 ? `faltando ${n} ${n === 1 ? "evento" : "eventos"}` : "não instalado";
}

type Action = "install" | "uninstall";

export function HooksSection() {
  const [status, setStatus] = useState<HooksStatus | null>(null);
  const [preview, setPreview] = useState<{ action: Action; report: HooksReport } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try { setStatus(await api.getHooksStatus()); } catch (e) { setMsg(`Falhou: ${e}`); }
  }, []);
  useEffect(() => { void load(); }, [load]);

  const run = (action: Action, dryRun: boolean) =>
    action === "install" ? api.installHooks(dryRun) : api.uninstallHooks(dryRun);

  async function doPreview(action: Action) {
    setBusy(true); setMsg(null); setConfirming(false);
    try { setPreview({ action, report: await run(action, true) }); }
    catch (e) { setMsg(`Falhou: ${e}`); }
    setBusy(false);
  }

  async function apply() {
    if (!preview) return;
    setBusy(true);
    try {
      const r = await run(preview.action, false);
      setStatus(r.status);
      setPreview(null); setConfirming(false);
      setMsg(r.changed ? `Feito. Backup em ${r.backupPath ?? "(sem backup)"}` : "Nada mudou.");
    } catch (e) { setMsg(`Falhou: ${e}`); }
    setBusy(false);
  }

  const nothingToRemove = !!status && !status.installed && status.missingEvents.length >= 7;
  const verb = preview?.action === "install" ? "instalar" : "remover";

  return (
    <Card tone={status && !status.installed && status.hookExeExists ? "primary" : "default"}>
      <CardTitle meta={status ? <span className="text-neon" role="status">{hooksSummary(status)}</span> : "lendo…"}>Claude Code</CardTitle>
      {status && (
        <>
          <p className="m-0 text-[10px] text-muted">
            {status.otherHooks === 0
              ? "Nenhum hook de outros apps."
              : `${status.otherHooks} ${status.otherHooks === 1 ? "hook de outro app será mantido" : "hooks de outros apps serão mantidos"}.`}
          </p>
          <p className="m-0 break-all text-[10px] text-muted">{status.settingsPath}</p>
          <div className="mt-2 flex flex-wrap gap-1.5">
            <Button size="sm" disabled={busy || !status.hookExeExists || status.installed} onClick={() => doPreview("install")}>
              Pré-visualizar instalação
            </Button>
            <Button size="sm" variant="ghost" disabled={busy || nothingToRemove} onClick={() => doPreview("uninstall")}>
              Remover
            </Button>
          </div>
        </>
      )}
      {preview && (
        <div className="mt-2">
          <div className="text-[10px] uppercase tracking-[0.12em] text-muted">Pré-visualização ({verb}), nada foi escrito</div>
          <pre className="mt-1 mb-0 max-h-40 overflow-auto border border-line bg-bg p-2 text-[10px] leading-[1.5] whitespace-pre-wrap">{preview.report.diff}</pre>
          {!confirming ? (
            <div className="mt-2 flex gap-1.5">
              <Button size="sm" variant="primary" disabled={!preview.report.changed} onClick={() => setConfirming(true)}>
                {preview.action === "install" ? "Instalar" : "Remover"}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setPreview(null)}>Cancelar</Button>
            </div>
          ) : (
            <div className="mt-2">
              <ConfirmCard confirmLabel={`Sim, ${verb}`} onConfirm={apply} onCancel={() => setConfirming(false)} busy={busy} label="Confirmar alteração">
                Isto altera {status?.settingsPath ?? "o settings.json"}. Um backup é criado antes. Confirmar?
              </ConfirmCard>
            </div>
          )}
        </div>
      )}
      {msg && <p className="mt-2 mb-0 break-all text-[11px] text-neon" role="status">{msg}</p>}
    </Card>
  );
}
