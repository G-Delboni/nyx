import { useEffect, useState } from "react";
import { api } from "../api/client";
import type { PlanPreset, Settings } from "../api/contract";
import { Button, Card, CardTitle, Field, Select, Slider, Toggle } from "../components";
import { HooksSection } from "../components/HooksSection";
import { useVoiceStatus } from "../hooks/useVoiceStatus";
import {
  AZURE_KEY_HINT, AZURE_VOICES, ENGINES, SPEAK_ON, TEST_PHRASE, clampOffset, clampVolume, engineLabel, formatChars,
  formatOffset, formatVolume, silenceReason,
} from "../voice/logic";

// Provisional weekly ceilings in estimated US$ (spec §6: calibration is an open question).
const PRESET_CEILING: Record<Exclude<PlanPreset, "custom">, number> = { pro: 230, max5x: 1150, max20x: 4600 };

export function SettingsTab({ settings, onSaved }: { settings: Settings; onSaved: () => void }) {
  const [s, setS] = useState<Settings>(settings);
  const [msg, setMsg] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testMsg, setTestMsg] = useState<string | null>(null);
  const voice = useVoiceStatus();
  useEffect(() => setS(settings), [settings]);

  const dirty = JSON.stringify(s) !== JSON.stringify(settings);
  const set = (p: Partial<Settings>) => { setS({ ...s, ...p }); setMsg(null); };

  function preset(p: PlanPreset) {
    set(p === "custom" ? { planPreset: p } : { planPreset: p, weeklyCeilingUsd: PRESET_CEILING[p] });
  }

  async function save() {
    try {
      await api.saveSettings(s);
      setMsg("Salvo.");
      onSaved();
    } catch (e) { setMsg(`Falhou: ${e}`); }
  }

  async function testVoice() {
    setTesting(true);
    setTestMsg(null);
    try {
      await api.testVoice({ text: TEST_PHRASE, voiceName: s.voiceName });
      void voice.refresh();
    } catch (e) { setTestMsg(`Falhou: ${e}`); }
    setTesting(false);
  }

  const vs = voice.status;
  const silence = silenceReason(s);
  const knownVoice = AZURE_VOICES.some((v) => v.id === s.voiceName);

  const [t1, t2] = [s.alertThresholds[0] ?? 80, s.alertThresholds[1] ?? 95];
  const sched = s.dndSchedule ?? { start: "22:00", end: "08:00" };

  return (
    <form className="flex flex-col gap-2" onSubmit={(e) => { e.preventDefault(); void save(); }}>
      <HooksSection />

      <Card>
        <CardTitle>Cena</CardTitle>
        <Select label="Topo do painel" value={s.sceneMode} onChange={(e) => set({ sceneMode: e.target.value as Settings["sceneMode"] })}
          hint={s.sceneMode === "avatar3d" ? "Beta: carrega o modelo 3D só com o painel aberto; se falhar, volta ao retrato." : "O retrato em penumbra (direção E)."}>
          <option value="portrait">Retrato</option>
          <option value="avatar3d">Avatar 3D (beta)</option>
        </Select>
        <Select label="Efeito ASCII" className="mt-2" value={s.asciiMode} disabled={s.sceneMode !== "avatar3d"}
          onChange={(e) => set({ asciiMode: e.target.value as Settings["asciiMode"] })}
          hint={s.sceneMode !== "avatar3d" ? "Só no avatar 3D; o retrato não tem ASCII." : s.asciiMode === "off" ? "Sem o pass de pós-processamento: a cena abre com as luzes e o alerta vira só o pulso de neon." : s.asciiMode === "introOnly" ? "A cena nasce em ASCII e resolve para o 3D; depois, nada." : "Abertura, fundo em ASCII sutil e o glitch do alerta."}>
          <option value="on">Sempre</option>
          <option value="introOnly">Só na abertura</option>
          <option value="off">Desligado</option>
        </Select>
      </Card>

      <Card>
        <CardTitle meta={vs ? engineLabel(vs.engineInUse) : "--"}>Voz</CardTitle>
        <div className="flex flex-col gap-2">
          <Toggle checked={s.voiceEnabled} onChange={(v) => set({ voiceEnabled: v })} label="Nyx fala" />
          <Toggle checked={s.voiceMuted} onChange={(v) => set({ voiceMuted: v })} label="Mudo" hint="Atalho rápido, também no menu da bandeja." />
          {silence && <p className="m-0 text-[10px] text-neon" role="status">{silence}</p>}
          <Select label="Motor" value={s.voiceEngine} onChange={(e) => set({ voiceEngine: e.target.value as Settings["voiceEngine"] })}
            hint="Automático usa Azure com chave e rede; sem isso, a voz do Windows.">
            {ENGINES.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
          </Select>
          <Select label="Voz Azure" value={s.voiceName} disabled={s.voiceEngine === "windows"} onChange={(e) => set({ voiceName: e.target.value })}>
            {!knownVoice && <option value={s.voiceName}>{s.voiceName}</option>}
            {AZURE_VOICES.map((v) => <option key={v.id} value={v.id}>{v.label}</option>)}
          </Select>
          <Slider label="Volume" min={0} max={1} step={0.05} value={s.voiceVolume} format={formatVolume}
            onChange={(v) => set({ voiceVolume: clampVolume(v) })} />
          <Slider label="Velocidade" min={-20} max={20} step={1} value={s.voiceRate} format={formatOffset}
            onChange={(v) => set({ voiceRate: clampOffset(v) })} />
          <Slider label="Tom" min={-20} max={20} step={1} value={s.voicePitch} format={formatOffset}
            onChange={(v) => set({ voicePitch: clampOffset(v) })} />
          <div className="flex flex-col gap-1.5">
            <span className="text-[10px] uppercase tracking-[0.12em] text-muted">Onde ela fala</span>
            {SPEAK_ON.map((o) => (
              <Toggle key={o.key} checked={s.voiceSpeakOn[o.key]} label={o.label}
                onChange={(v) => set({ voiceSpeakOn: { ...s.voiceSpeakOn, [o.key]: v } })} />
            ))}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button type="button" onClick={() => void testVoice()} disabled={testing}>Testar voz</Button>
            <Button type="button" variant="ghost" onClick={() => void api.stopSpeech()}>Parar</Button>
            {testMsg && <span role="alert" className="text-[11px] text-neon">{testMsg}</span>}
          </div>
          {vs && (
            <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11px]">
              <dt className="text-muted">motor em uso</dt><dd className="m-0 text-text">{engineLabel(vs.engineInUse)}</dd>
              <dt className="text-muted">chave Azure</dt>
              <dd className="m-0 text-text">{vs.azureKeyPresent ? `sim${vs.azureRegion ? ` · ${vs.azureRegion}` : ""}` : "não"}</dd>
              <dt className="text-muted">voz do Windows</dt><dd className="m-0 text-text">{vs.windowsVoice ?? "não encontrada"}</dd>
              <dt className="text-muted">caracteres no mês</dt>
              <dd className="m-0 text-text">{formatChars(vs.monthChars)} de {formatChars(vs.freeTierChars)}</dd>
              {vs.lastError && (<><dt className="text-muted">último erro</dt><dd className="m-0 break-words text-neon">{vs.lastError}</dd></>)}
            </dl>
          )}
          {vs && !vs.azureKeyPresent && <p className="m-0 text-[10px] text-muted">Sem chave Azure: {AZURE_KEY_HINT}.</p>}
        </div>
      </Card>

      <Card>
        <CardTitle>Plano e teto</CardTitle>
        <div className="flex flex-col gap-2">
          <Select label="Preset do plano" value={s.planPreset} onChange={(e) => preset(e.target.value as PlanPreset)}>
            <option value="pro">Pro</option>
            <option value="max5x">Max 5x</option>
            <option value="max20x">Max 20x</option>
            <option value="custom">Personalizado</option>
          </Select>
          <Field label="Teto semanal (US$, estimativa)" type="number" min={1} step={1} value={s.weeklyCeilingUsd}
            onChange={(e) => set({ planPreset: "custom", weeklyCeilingUsd: Math.max(1, Number(e.target.value) || 1) })} />
          <div className="grid grid-cols-2 gap-2">
            <Field label="Alerta 1 (%)" type="number" min={1} max={100} value={t1}
              onChange={(e) => set({ alertThresholds: [Number(e.target.value), t2] })} />
            <Field label="Alerta 2 (%)" type="number" min={1} max={100} value={t2}
              onChange={(e) => set({ alertThresholds: [t1, Number(e.target.value)] })} />
          </div>
        </div>
      </Card>

      <Card>
        <CardTitle>Não perturbe</CardTitle>
        <div className="flex flex-col gap-2">
          <Toggle checked={s.doNotDisturb} onChange={(v) => set({ doNotDisturb: v })} label="Ligado agora" />
          <div className="grid grid-cols-2 gap-2">
            <Field label="Das" type="time" value={sched.start} onChange={(e) => set({ dndSchedule: { ...sched, start: e.target.value } })} />
            <Field label="Até" type="time" value={sched.end} onChange={(e) => set({ dndSchedule: { ...sched, end: e.target.value } })} />
          </div>
          <p className="m-0 text-[10px] text-muted">Prioridade média vira só badge. Alta continua passando.</p>
        </div>
      </Card>

      <Card>
        <CardTitle>Sistema</CardTitle>
        <div className="flex flex-col gap-2">
          <Toggle checked={s.autostart} onChange={(v) => set({ autostart: v })} label="Iniciar com o Windows (na bandeja)" />
          <Field label="Pasta de dados" readOnly value={s.dataDir} hint="Somente leitura." spellCheck={false} />
        </div>
      </Card>

      <div className="flex items-center gap-3">
        <Button type="submit" variant="primary" disabled={!dirty}>Salvar</Button>
        {msg && <span role="status" className="text-[11px] text-neon">{msg}</span>}
      </div>
    </form>
  );
}
