//! Chat com a Nyx (docs/specs/NOITE-2.md §4): histórico por contexto no SQLite, o
//! resumo local compacto que vai para o Haiku e o mapa tool use -> cartão de ação.
//! As ações só rodam com o clique do usuário (`run_chat_action`).

use crate::ai::{AiBackend, ChatTurn, ProposedAction, Role};
use crate::db::Db;
use crate::model::*;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

pub const HISTORY_FOR_AI: u32 = 20;
pub const MAX_CHAT_CHARS: usize = 4_000;

fn rfc(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn map_msg(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessage> {
    let role: String = r.get(2)?;
    let actions: String = r.get(4)?;
    Ok(ChatMessage {
        id: r.get(0)?,
        context_id: r.get(1)?,
        role: if role == "user" { ChatRole::User } else { ChatRole::Nyx },
        text: r.get(3)?,
        actions: serde_json::from_str(&actions).unwrap_or_default(),
        cost_usd: r.get(5)?,
        created_at: r.get(6)?,
    })
}

const SELECT: &str = "SELECT id, context_id, role, text, actions, cost_usd, created_at FROM chat_messages";

pub fn insert(
    conn: &Connection,
    context_id: Option<&str>,
    role: ChatRole,
    text: &str,
    actions: &[ChatAction],
    cost: f64,
    at: DateTime<Utc>,
) -> Result<ChatMessage> {
    conn.execute(
        "INSERT INTO chat_messages (context_id, role, text, actions, cost_usd, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            context_id,
            if role == ChatRole::User { "user" } else { "nyx" },
            text,
            serde_json::to_string(actions)?,
            cost,
            rfc(&at)
        ],
    )?;
    get(conn, conn.last_insert_rowid())?.ok_or_else(|| anyhow!("mensagem sumiu"))
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<ChatMessage>> {
    Ok(conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], map_msg).optional()?)
}

/// Últimas `limit` mensagens do contexto (null = geral), em ordem cronológica.
pub fn history(conn: &Connection, context_id: Option<&str>, limit: u32) -> Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE context_id IS ?1 ORDER BY id DESC LIMIT ?2"
    ))?;
    let mut v: Vec<ChatMessage> =
        stmt.query_map(params![context_id, limit], map_msg)?.collect::<rusqlite::Result<_>>()?;
    v.reverse();
    Ok(v)
}

pub fn clear(conn: &Connection, context_id: Option<&str>) -> Result<()> {
    conn.execute("DELETE FROM chat_messages WHERE context_id IS ?1", params![context_id])?;
    Ok(())
}

pub fn update_actions(conn: &Connection, id: i64, actions: &[ChatAction]) -> Result<()> {
    conn.execute("UPDATE chat_messages SET actions = ?2 WHERE id = ?1", params![id, serde_json::to_string(actions)?])?;
    Ok(())
}

/// Proposta do Haiku (`nyx-ai`) -> cartão. Tipos desconhecidos são descartados.
pub fn actions_from(proposals: &[ProposedAction]) -> Vec<ChatAction> {
    proposals
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let kind: ChatActionKind = serde_json::from_value(Value::String(p.kind.clone())).ok()?;
            let mut args = p.args.clone();
            args.remove("label");
            Some(ChatAction {
                id: if p.id.is_empty() { format!("a{}", i + 1) } else { p.id.clone() },
                kind,
                label: p.label.clone(),
                args,
                state: ChatActionState::Proposed,
            })
        })
        .collect()
}

/// Resumo local compacto que acompanha o pedido (nada de transcripts nem código).
pub fn snapshot_text(snap: &Snapshot, delegations: &[Delegation], events: &[NyxEvent], context: Option<&ProjectContext>) -> String {
    let mut s = String::new();
    if let Some(c) = context {
        s.push_str(&format!("Contexto atual: {} (id {})\n", c.name, c.id));
    } else {
        s.push_str("Contexto atual: geral\n");
    }
    s.push_str(&format!("Status: {:?}\n", snap.status).to_lowercase());
    let w = &snap.week_usage;
    s.push_str(&format!(
        "Uso da semana: US$ {:.2}{}\n",
        w.cost_usd,
        match (w.ceiling_usd, w.percent) {
            (Some(c), Some(p)) => format!(" de US$ {c:.0} estimados ({p:.0}%)"),
            _ => String::new(),
        }
    ));
    s.push_str("Agentes:\n");
    for a in snap.agents.iter().take(8) {
        // Local-first: sem texto vindo dos transcripts (last_message), só pasta e status.
        s.push_str(&format!("- {} [{}]\n", a.folder_name.as_deref().unwrap_or("?"), a.status.as_str()));
    }
    s.push_str("Delegações:\n");
    for d in delegations.iter().take(5) {
        s.push_str(&format!(
            "- id {} \"{}\" [{}] etapa {} de {}\n",
            d.id,
            d.title,
            d.status.as_str(),
            d.current_step + 1,
            d.steps.len()
        ));
    }
    s.push_str("Últimos eventos:\n");
    for e in events.iter().take(5) {
        s.push_str(&format!("- {} {}\n", e.created_at, e.title));
    }
    s
}

/// Pergunta -> resposta: grava as duas mensagens e o custo. Sem chave, responde localmente.
pub fn send(
    db: &std::sync::Mutex<Db>,
    ai: &dyn AiBackend,
    context_id: Option<&str>,
    text: &str,
    snapshot: &str,
    now: DateTime<Utc>,
) -> Result<(ChatMessage, ChatMessage)> {
    let text = text.trim();
    if text.is_empty() {
        anyhow::bail!("Mensagem vazia.");
    }
    if text.chars().count() > MAX_CHAT_CHARS {
        anyhow::bail!("Mensagem longa demais (máximo {MAX_CHAT_CHARS} caracteres).");
    }
    let user = insert(db.lock().unwrap().conn(), context_id, ChatRole::User, text, &[], 0.0, now)?;
    if !ai.available() {
        let reply = insert(
            db.lock().unwrap().conn(),
            context_id,
            ChatRole::Nyx,
            "Sem a chave da API não converso, Dev. Defina NYX_ANTHROPIC_API_KEY nas variáveis de ambiente e reinicie a Nyx. O resto continua funcionando.",
            &[],
            0.0,
            now,
        )?;
        return Ok((user, reply));
    }
    let mut hist = history(db.lock().unwrap().conn(), context_id, HISTORY_FOR_AI + 1)?;
    hist.retain(|m| m.id != user.id); // a pergunta nova vai separada
    let turns: Vec<ChatTurn> = hist
        .iter()
        .map(|m| ChatTurn { role: if m.role == ChatRole::User { Role::User } else { Role::Assistant }, text: m.text.clone() })
        .collect();
    let reply = match ai.chat(snapshot, &turns, text) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("chat: {e:#}");
            let m = insert(
                db.lock().unwrap().conn(),
                context_id,
                ChatRole::Nyx,
                &format!("Não consegui falar com a API agora. {}", first_line(&format!("{e:#}"))),
                &[],
                0.0,
                now,
            )?;
            return Ok((user, m));
        }
    };
    let actions = actions_from(&reply.actions);
    let db = db.lock().unwrap();
    db.add_ai_cost("chat", reply.cost_usd, now)?;
    let text = if reply.text.trim().is_empty() && !actions.is_empty() { "Posso fazer isso. Confirma?".to_string() } else { reply.text };
    let nyx = insert(db.conn(), context_id, ChatRole::Nyx, &text, &actions, reply.cost_usd, now)?;
    Ok((user, nyx))
}

/// O que um cartão confirmado pode fazer: os mesmos commands da UI.
pub trait ActionTarget {
    fn start_delegation(&self, draft: DelegationDraft) -> Result<()>;
    fn approve_step(&self, delegation_id: &str) -> Result<()>;
    fn adjust_step(&self, delegation_id: &str, feedback: &str) -> Result<()>;
    fn install_hooks(&self, dry_run: bool) -> Result<()>;
    fn create_context(&self, ctx: ProjectContext) -> Result<()>;
    /// `tab`: agents|delegate|usage|events|contexts|settings|chat (None = só abre).
    fn open_panel(&self, tab: Option<&str>) -> Result<()>;
    /// Para validar os args do modelo antes de rodar.
    fn delegation(&self, id: &str) -> Result<Delegation>;
    fn context_exists(&self, id: &str) -> bool;
}

fn arg_str(a: &Map<String, Value>, k: &str) -> Option<String> {
    a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn arg_list(a: &Map<String, Value>, k: &str) -> Vec<String> {
    a.get(k)
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Delegação do arg, no estado esperado e do mesmo contexto da conversa.
fn checked_delegation(
    target: &dyn ActionTarget,
    msg_ctx: Option<&str>,
    id: &str,
    states: &[DelegationStatus],
) -> Result<Delegation> {
    let d = target.delegation(id).map_err(|_| anyhow!("Delegação {id} não existe."))?;
    if let Some(c) = msg_ctx {
        if d.context_id != c {
            anyhow::bail!("Essa delegação é de outro contexto.");
        }
    }
    if !states.contains(&d.status) {
        anyhow::bail!("A delegação não está esperando isso (está em {}).", d.status.as_str());
    }
    Ok(d)
}

/// Pasta de contexto proposta pelo modelo: absoluta, existente, não a raiz do disco nem a home.
fn checked_root(raw: &str) -> Result<String> {
    let p = std::path::Path::new(raw);
    if !p.is_absolute() {
        anyhow::bail!("A pasta precisa ser um caminho absoluto.");
    }
    let c = std::fs::canonicalize(p).map_err(|_| anyhow!("A pasta {raw} não existe."))?;
    if !c.is_dir() {
        anyhow::bail!("{raw} não é uma pasta.");
    }
    if c.parent().is_none() || c.parent().and_then(|x| x.parent()).is_none() && raw.trim_end_matches(['\\', '/']).ends_with(':') {
        anyhow::bail!("Não uso a raiz do disco como contexto.");
    }
    if let Some(h) = dirs::home_dir().and_then(|h| std::fs::canonicalize(h).ok()) {
        if c == h {
            anyhow::bail!("Não uso a pasta pessoal inteira como contexto.");
        }
    }
    Ok(raw.to_string())
}

fn execute(target: &dyn ActionTarget, msg_ctx: Option<&str>, a: &ChatAction) -> Result<()> {
    use DelegationStatus as S;
    let need = |k: &str| arg_str(&a.args, k).ok_or_else(|| anyhow!("faltou {k} na ação"));
    match a.kind {
        ChatActionKind::StartDelegation => {
            let context_id = arg_str(&a.args, "contextId")
                .or_else(|| msg_ctx.map(str::to_string))
                .ok_or_else(|| anyhow!("faltou o contexto da delegação"))?;
            if !target.context_exists(&context_id) {
                anyhow::bail!("Contexto {context_id} não existe.");
            }
            // modelo, IA e ferramentas nunca vêm do modelo: padrões fixos (o usuário muda na UI)
            target.start_delegation(DelegationDraft {
                context_id,
                title: need("title")?,
                steps: arg_list(&a.args, "steps"),
                use_ai: false,
                model: None,
            })
        }
        ChatActionKind::ApproveStep => {
            let d = checked_delegation(target, msg_ctx, &need("delegationId")?, &[S::Checkpoint])?;
            target.approve_step(&d.id)
        }
        ChatActionKind::AdjustStep => {
            let d = checked_delegation(target, msg_ctx, &need("delegationId")?, &[S::Checkpoint, S::Blocked, S::Failed])?;
            target.adjust_step(&d.id, &need("feedback")?)
        }
        ChatActionKind::InstallHooks => {
            target.install_hooks(a.args.get("dryRun").and_then(Value::as_bool).unwrap_or(false))
        }
        // Só nome e pasta: ferramentas, instruções, referências e IA o usuário define na UI.
        ChatActionKind::CreateContext => target.create_context(ProjectContext {
            name: need("name")?,
            root_path: checked_root(&need("rootPath")?)?,
            ..Default::default()
        }),
        ChatActionKind::OpenPanel => target.open_panel(arg_str(&a.args, "tab").as_deref()),
    }
}

/// O usuário confirmou um cartão: roda a ação (uma vez) e grava o estado dela.
pub fn run_action(db: &std::sync::Mutex<Db>, message_id: i64, action_id: &str, target: &dyn ActionTarget) -> Result<ChatMessage> {
    let msg = get(db.lock().unwrap().conn(), message_id)?.ok_or_else(|| anyhow!("Mensagem não encontrada."))?;
    let idx = msg
        .actions
        .iter()
        .position(|a| a.id == action_id)
        .ok_or_else(|| anyhow!("Ação não encontrada."))?;
    if msg.actions[idx].state != ChatActionState::Proposed {
        anyhow::bail!("Essa ação já foi usada.");
    }
    // Clique duplo: só um chamador por (mensagem, ação) passa daqui.
    static IN_FLIGHT: std::sync::Mutex<Vec<(i64, String)>> = std::sync::Mutex::new(Vec::new());
    let key = (message_id, action_id.to_string());
    {
        let mut f = IN_FLIGHT.lock().unwrap();
        if f.contains(&key) {
            anyhow::bail!("Essa ação já está rodando.");
        }
        f.push(key.clone());
    }
    struct Release((i64, String));
    impl Drop for Release {
        fn drop(&mut self) {
            IN_FLIGHT.lock().unwrap().retain(|k| *k != self.0);
        }
    }
    let _release = Release(key);
    // relê depois de reservar: outro clique pode ter terminado antes
    let mut msg = get(db.lock().unwrap().conn(), message_id)?.ok_or_else(|| anyhow!("Mensagem não encontrada."))?;
    if msg.actions[idx].state != ChatActionState::Proposed {
        anyhow::bail!("Essa ação já foi usada.");
    }
    let result = execute(target, msg.context_id.as_deref(), &msg.actions[idx]);
    msg.actions[idx].state = if result.is_ok() { ChatActionState::Done } else { ChatActionState::Failed };
    update_actions(db.lock().unwrap().conn(), message_id, &msg.actions)?;
    result?;
    Ok(msg)
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{fake::FakeAi, ChatReply};
    use serde_json::json;
    use std::sync::Mutex;

    #[test]
    fn history_per_context_and_clear() {
        let db = Db::open_in_memory().unwrap();
        let t = Utc::now();
        insert(db.conn(), None, ChatRole::User, "geral", &[], 0.0, t).unwrap();
        insert(db.conn(), Some("c1"), ChatRole::User, "a", &[], 0.0, t).unwrap();
        insert(db.conn(), Some("c1"), ChatRole::Nyx, "b", &[], 0.01, t).unwrap();
        let h = history(db.conn(), Some("c1"), 10).unwrap();
        assert_eq!(h.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(history(db.conn(), None, 10).unwrap().len(), 1);
        assert_eq!(history(db.conn(), Some("c1"), 1).unwrap()[0].text, "b");
        clear(db.conn(), Some("c1")).unwrap();
        assert!(history(db.conn(), Some("c1"), 10).unwrap().is_empty());
        assert_eq!(history(db.conn(), None, 10).unwrap().len(), 1);
    }

    #[test]
    fn without_key_replies_locally() {
        let db = Mutex::new(Db::open_in_memory().unwrap());
        let off = FakeAi { off: true, ..Default::default() };
        let (_, r) = send(&db, &off, None, "oi", "", Utc::now()).unwrap();
        assert!(r.text.contains("NYX_ANTHROPIC_API_KEY"));
        assert!(!r.text.contains('!'));
        assert_eq!(r.cost_usd, 0.0);
    }

    #[test]
    fn reply_with_proposed_action() {
        let db = Mutex::new(Db::open_in_memory().unwrap());
        let ai = FakeAi::default();
        ai.replies.lock().unwrap().push(ChatReply {
            text: "Dá para aprovar a etapa 2.".into(),
            actions: vec![
                ProposedAction {
                    id: "toolu_1".into(),
                    kind: "approveStep".into(),
                    label: "Aprovar etapa 2".into(),
                    args: json!({"delegationId": "d1"}).as_object().unwrap().clone(),
                },
                ProposedAction { id: "toolu_2".into(), kind: "rmRf".into(), label: "x".into(), args: Default::default() },
            ],
            cost_usd: 0.003,
        });
        let (_, r) = send(&db, &ai, Some("c1"), "aprova?", "snap", Utc::now()).unwrap();
        assert_eq!(r.actions.len(), 1);
        assert_eq!(r.actions[0].kind, ChatActionKind::ApproveStep);
        assert_eq!(r.actions[0].label, "Aprovar etapa 2");
        assert_eq!(r.actions[0].args["delegationId"], "d1");
        assert_eq!(r.actions[0].id, "toolu_1");
        assert_eq!(r.actions[0].state, ChatActionState::Proposed);
        let db = db.lock().unwrap();
        assert!((db.ai_cost_since(Utc::now() - chrono::Duration::days(7)).unwrap() - 0.003).abs() < 1e-9);
        assert_eq!(get(db.conn(), r.id).unwrap().unwrap(), r);
        let calls = ai.calls.lock().unwrap();
        assert!(calls[0].ends_with("|0|aprova?"), "{}", calls[0]); // histórico sem a pergunta
    }
}
