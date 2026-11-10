import { useCallback, useEffect, useState } from "react";
import { api, subscribe } from "../api/client";
import type { ChatMessage } from "../api/contract";
import { inContext, mergeMessage } from "../chat/messages";

/** Chat history of one context (null = general), live through nyx://chat. */
export function useChat(contextId: string | null) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setMessages(await api.getChat(contextId, 50));
    } catch (e) {
      setError(String(e));
    }
  }, [contextId]);

  useEffect(() => {
    setMessages([]);
    setError(null);
    void reload();
    let alive = true;
    let un: (() => void) | null = null;
    void subscribe.chat((m) => {
      if (inContext(m, contextId)) setMessages((l) => mergeMessage(l.filter((x) => x.id >= 0), m));
    }).then((u) => (alive ? (un = u) : u()));
    return () => { alive = false; un?.(); };
  }, [contextId, reload]);

  const send = useCallback(async (text: string) => {
    const t = text.trim();
    if (!t) return;
    setSending(true);
    setError(null);
    // Optimistic user bubble (negative id); the authoritative list replaces it.
    setMessages((l) => [...l, { id: -1, contextId, role: "user", text: t, actions: [], costUsd: 0, createdAt: new Date().toISOString() }]);
    try {
      await api.sendChat(contextId, t);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      await reload();
      setSending(false);
    }
  }, [contextId, reload]);

  const clear = useCallback(async () => {
    try {
      await api.clearChat(contextId);
      setMessages([]);
    } catch (e) {
      setError(String(e));
    }
  }, [contextId]);

  const runAction = useCallback(async (messageId: number, actionId: string) => {
    try {
      const m = await api.runChatAction(messageId, actionId);
      setMessages((l) => mergeMessage(l, m));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  return { messages, sending, error, send, clear, runAction };
}
