import { useState } from "react";
import { api } from "../api/client";
import type { ProjectContext } from "../api/contract";
import { Button, Card, ConfirmCard, EmptyState, Field, TextArea, Toggle } from "../components";
import { detectPreset, formatLines, formatTools, invalidTools, isBroadBash, parseLines, parseTools, TOOL_PRESETS } from "../delegation/tools";

const blank = (): ProjectContext => ({ id: "", name: "", rootPath: "", instructions: "", color: null, createdAt: "", allowedTools: ["Read", "Grep", "Glob"], references: [], aiEnabled: false });

export function ContextsTab({ contexts, onChanged }: { contexts: ProjectContext[]; onChanged: () => void }) {
  const [draft, setDraft] = useState<ProjectContext | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [removing, setRemoving] = useState<ProjectContext | null>(null);
  const [toolsText, setToolsText] = useState("");
  const [refsText, setRefsText] = useState("");

  function edit(c: ProjectContext | null) {
    setDraft(c);
    setToolsText(c ? formatTools(c.allowedTools) : "");
    setRefsText(c ? formatLines(c.references) : "");
  }

  async function save() {
    if (!draft) return;
    if (!draft.name.trim() || !draft.rootPath.trim()) { setErr("Nome e pasta raiz são obrigatórios."); return; }
    const bad = invalidTools(draft.allowedTools);
    if (bad.length) { setErr(`Ferramenta inválida: ${bad.join(", ")}. Use Nome ou Nome(padrão).`); return; }
    try {
      await api.saveContext({ ...draft, name: draft.name.trim(), rootPath: draft.rootPath.trim() });
      edit(null); setErr(null); onChanged();
    } catch (e) { setErr(String(e)); }
  }

  async function remove(c: ProjectContext) {
    try { await api.deleteContext(c.id); setRemoving(null); onChanged(); } catch (e) { setErr(String(e)); }
  }

  if (draft) {
    const set = (p: Partial<ProjectContext>) => setDraft({ ...draft, ...p });
    return (
      <form className="flex flex-col gap-2.5" onSubmit={(e) => { e.preventDefault(); void save(); }}>
        <span className="text-[10px] uppercase tracking-[0.14em] text-primary">{draft.id ? "Editar contexto" : "Novo contexto"}</span>
        <Field label="Nome" value={draft.name} onChange={(e) => set({ name: e.target.value })} autoFocus required />
        <Field label="Pasta raiz" value={draft.rootPath} placeholder="C:\Users\…\projeto" spellCheck={false} autoComplete="off" onChange={(e) => set({ rootPath: e.target.value })} required />
        <TextArea label="Instruções permanentes" rows={5} value={draft.instructions} onChange={(e) => set({ instructions: e.target.value })}
          hint="Entram em todo prompt delegado neste contexto." />
        <fieldset className="m-0 flex flex-col gap-1.5 border-0 p-0">
          <legend className="mb-1 p-0 text-[10px] uppercase tracking-[0.12em] text-muted">Ferramentas permitidas</legend>
          <div className="flex flex-wrap gap-1.5" role="group" aria-label="Presets de ferramentas">
            {TOOL_PRESETS.map((p) => (
              <Button key={p.id} size="sm" variant={detectPreset(draft.allowedTools)?.id === p.id ? "primary" : "outline"}
                aria-pressed={detectPreset(draft.allowedTools)?.id === p.id}
                onClick={() => { set({ allowedTools: [...p.tools] }); setToolsText(formatTools(p.tools)); }}>{p.label}</Button>
            ))}
          </div>
          <TextArea label="Lista (uma por linha)" rows={5} value={toolsText} spellCheck={false}
            onChange={(e) => { setToolsText(e.target.value); set({ allowedTools: parseTools(e.target.value) }); }}
            hint={draft.allowedTools.some(isBroadBash)
              ? "Atenção: Bash sem padrão libera qualquer comando. Prefira Bash(npm test:*)."
              : "Fora da lista, o passo para como bloqueado e você decide. A lista vira --allowedTools."} />
        </fieldset>
        <TextArea label="Referências (uma por linha)" rows={3} value={refsText} spellCheck={false} placeholder="docs/PLANO.md"
          onChange={(e) => { setRefsText(e.target.value); set({ references: parseLines(e.target.value) }); }}
          hint="Arquivos ou notas injetados em todo prompt delegado." />
        <Toggle checked={draft.aiEnabled} onChange={(aiEnabled) => set({ aiEnabled })} label="IA neste contexto"
          hint="Resumos e planos com Haiku. Precisa de NYX_ANTHROPIC_API_KEY. Padrão das novas delegações." />
        {err && <p className="m-0 text-neon" role="alert">{err}</p>}
        <div className="flex gap-1.5">
          <Button type="submit" variant="primary">Salvar contexto</Button>
          <Button variant="ghost" onClick={() => { edit(null); setErr(null); }}>Cancelar</Button>
        </div>
      </form>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <div><Button onClick={() => edit(blank())}>+ Novo contexto</Button></div>
      {err && <p className="m-0 text-neon" role="alert">{err}</p>}
      {contexts.length === 0 && <EmptyState title="Contextos">Sem contextos. Crie um para agrupar sessões por pasta.</EmptyState>}
      <ul className="m-0 flex list-none flex-col gap-2 p-0">
        {contexts.map((c) => (
          <li key={c.id}>
            <Card>
              <div className="font-display text-base leading-[1.1] font-semibold">{c.name}</div>
              <div className="mt-0.5 break-all text-[10px] tracking-[0.04em] text-muted">{c.rootPath}</div>
              {c.instructions && <p className="mt-1.5 mb-0 line-clamp-2 text-[11px] text-muted">{c.instructions}</p>}
              <div className="mt-2 flex flex-wrap gap-1.5 text-[10px] tracking-[0.06em] text-muted">
                <span>{c.allowedTools.length} {c.allowedTools.length === 1 ? "ferramenta" : "ferramentas"}</span>
                <span>· IA {c.aiEnabled ? "ligada" : "desligada"}</span>
              </div>
              {removing?.id === c.id ? (
                <div className="mt-2">
                  <ConfirmCard tone="danger" confirmLabel="Apagar" onConfirm={() => remove(c)} onCancel={() => setRemoving(null)} label={`Apagar ${c.name}`}>
                    Apagar o contexto "{c.name}"? As sessões continuam existindo.
                  </ConfirmCard>
                </div>
              ) : (
                <div className="mt-2 flex gap-1.5">
                  <Button size="sm" onClick={() => edit(c)}>Editar</Button>
                  <Button size="sm" variant="ghost" onClick={() => setRemoving(c)}>Apagar</Button>
                </div>
              )}
            </Card>
          </li>
        ))}
      </ul>
    </div>
  );
}
