import { useEffect, useRef, useState } from "react";
import type { ChatAction, ChatMessage, Snapshot } from "../api/contract";
import { Button, ChatBubble, ConfirmCard, EmptyState, Select, TextArea } from "../components";
import { ACTION_STATE_LABEL, describeAction, MAX_CHAT_CHARS, NO_KEY_HELP, replyCost, sendBlock } from "../chat/messages";
import { useAiStatus } from "../hooks/useAiStatus";
import { useChat } from "../hooks/useChat";
import { formatUsdFine, relativeTime } from "../lib/format";

export function ChatTab({ snap, now }: { snap: Snapshot; now: number }) {
  const [ctxId, setCtxId] = useState<string | null>(snap.contexts[0]?.id ?? null);
  const { status, keyPresent } = useAiStatus();
  const { messages, sending, error, send, clear, runAction } = useChat(ctxId);
  const [text, setText] = useState("");
  const [confirming, setConfirming] = useState<{ messageId: number; action: ChatAction } | null>(null);
  const [clearing, setClearing] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => { endRef.current?.scrollIntoView?.({ block: "end" }); }, [messages.length, sending]);

  const block = sendBlock(text, { keyPresent, sending });
  async function submit() {
    if (block) return;
    const t = text;
    setText("");
    await send(t);
  }

  return (
    <div className="flex min-h-full flex-col gap-2.5">
      <div className="flex items-end gap-2">
        <Select className="flex-1" label="Conversa" value={ctxId ?? ""} onChange={(e) => setCtxId(e.target.value || null)}>
          <option value="">Geral</option>
          {snap.contexts.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
        </Select>
        <Button size="sm" variant="ghost" disabled={messages.length === 0} onClick={() => setClearing(true)}>Limpar conversa</Button>
      </div>
      {clearing && (
        <ConfirmCard tone="danger" confirmLabel="Limpar" label="Limpar conversa" onCancel={() => setClearing(false)}
          onConfirm={() => { void clear(); setClearing(false); }}>
          Apaga o histórico desta conversa. Não dá para desfazer.
        </ConfirmCard>
      )}

      <div className="flex flex-1 flex-col gap-2.5" role="log" aria-label="Conversa com a Nyx" aria-live="polite">
        {!keyPresent && status && <EmptyState title="Nyx offline">{NO_KEY_HELP}</EmptyState>}
        {keyPresent && messages.length === 0 && <EmptyState title="Nyx">Online, Dev. Pergunte pelo uso, pelas sessões ou peça uma delegação.</EmptyState>}
        {messages.map((m) => (
          <Message key={m.id} m={m} now={now} onAsk={(action) => setConfirming({ messageId: m.id, action })}
            confirming={confirming?.messageId === m.id ? confirming.action : null}
            onConfirm={() => { if (confirming) { void runAction(confirming.messageId, confirming.action.id); setConfirming(null); } }}
            onCancel={() => setConfirming(null)} />
        ))}
        {sending && <p className="m-0 text-[10px] uppercase tracking-[0.12em] text-muted" role="status">Nyx está pensando…</p>}
        <div ref={endRef} />
      </div>

      {error && <p className="m-0 text-neon" role="alert">{error}</p>}
      <form className="flex flex-col gap-1.5" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <TextArea label="Mensagem" rows={2} value={text} disabled={!keyPresent} maxLength={MAX_CHAT_CHARS + 200}
          placeholder={keyPresent ? "Escreva para a Nyx…" : "Sem NYX_ANTHROPIC_API_KEY"}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void submit(); } }}
          hint={block === "tooLong" ? `Máximo ${MAX_CHAT_CHARS} caracteres.` : "Enter envia, Shift+Enter quebra a linha."} />
        <div className="flex items-center justify-between">
          <Button type="submit" variant="primary" disabled={block !== null}>Enviar</Button>
          <span className="text-[10px] tracking-[0.06em] text-muted" title="Custo da API da própria Nyx, separado do uso do Claude Code">
            IA · conversa {formatUsdFine(replyCost(messages))} · semana {status ? formatUsdFine(status.weekCostUsd) : "--"}
          </span>
        </div>
      </form>
    </div>
  );
}

function Message({ m, now, confirming, onAsk, onConfirm, onCancel }: {
  m: ChatMessage; now: number; confirming: ChatAction | null;
  onAsk: (a: ChatAction) => void; onConfirm: () => void; onCancel: () => void;
}) {
  const meta = `${relativeTime(m.createdAt, now)}${m.role === "nyx" && m.costUsd > 0 ? ` · ${formatUsdFine(m.costUsd)}` : ""}`;
  const actions = m.actions.length > 0 ? (
    <>
      {m.actions.map((a) =>
        confirming?.id === a.id ? (
          <ConfirmCard key={a.id} confirmLabel={a.label} onConfirm={onConfirm} onCancel={onCancel} label={a.label}>{describeAction(a)}</ConfirmCard>
        ) : a.state === "proposed" ? (
          <Button key={a.id} variant="primary" size="sm" onClick={() => onAsk(a)}>{a.label}</Button>
        ) : (
          <span key={a.id} className="text-[10px] uppercase tracking-[0.1em] text-muted">{a.label} · {ACTION_STATE_LABEL[a.state]}</span>
        ))}
    </>
  ) : undefined;
  return <ChatBubble role={m.role} meta={meta} actions={actions}>{m.text}</ChatBubble>;
}
