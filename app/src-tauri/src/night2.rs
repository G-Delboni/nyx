//! Cola Tauri da Noite 2: o sink da delegação (eventos, toasts com Aprovar), o alvo
//! das ações do chat e os commands de delegação/chat/IA. As operações que podem
//! demorar (git, API) rodam fora da thread principal (`spawn_blocking`).

use crate::ai::AiBackend;
use crate::chat::{self, ActionTarget};
use crate::db::NewEvent;
use crate::delegation::{self, DelegSink, Engine};
use crate::model::*;
use crate::toast::Toast;
use crate::{core, AppState};
use chrono::Utc;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::{Action, ActionType, NotificationExt};

pub const EVT_DELEGATION: &str = "nyx://delegation";
pub const EVT_CHAT: &str = "nyx://chat";

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Roda `f` numa thread de bloqueio com acesso ao app.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppHandle) -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app)).await.map_err(err)?.map_err(|e| format!("{e:#}"))
}

fn engine(app: &AppHandle) -> Arc<Engine> {
    app.state::<AppState>().deleg.clone()
}

fn ai(app: &AppHandle) -> Arc<dyn AiBackend> {
    app.state::<AppState>().ai.clone()
}

// ---------- sink ----------

pub struct TauriSink {
    pub app: AppHandle,
}

impl TauriSink {
    fn record(&self, d: &Delegation, kind: EventKind, priority: Priority, title: &str, detail: Option<String>) -> Option<i64> {
        let state = self.app.state::<AppState>();
        let ev = state.db.lock().unwrap().insert_event(&NewEvent {
            kind,
            priority,
            session_id: d.claude_session_id.clone(),
            context_id: Some(d.context_id.clone()),
            title: title.to_string(),
            detail,
            created_at: Utc::now(),
            raw_hook: Some(format!("delegation:{}", d.id)),
        });
        match ev {
            Ok(ev) => {
                let _ = self.app.emit(crate::EVT_EVENT, &ev);
                Some(ev.id)
            }
            Err(e) => {
                log::warn!("evento da delegação: {e:#}");
                None
            }
        }
    }
}

fn report_text(s: &DelegationStep) -> Option<String> {
    s.report.as_ref().map(|r| {
        let mut t = r.title.clone();
        for l in &r.lines {
            t.push('\n');
            t.push_str(l);
        }
        t
    })
}

impl DelegSink for TauriSink {
    fn changed(&self, d: &Delegation) {
        let _ = self.app.emit(EVT_DELEGATION, d);
    }

    fn checkpoint(&self, d: &Delegation, step: &DelegationStep) {
        let title = delegation::checkpoint_title(d);
        let id = self.record(d, EventKind::TaskDone, Priority::High, &title, report_text(step));
        let body = step.report.as_ref().map(|r| r.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| d.title.clone());
        let speech = crate::toast::speech(SpeechKind::Alert, title.clone());
        notify_delegation(&self.app, &Toast { title, body, priority: Priority::High, event_id: id, speech }, &d.id, true);
    }

    fn blocked(&self, d: &Delegation) {
        let reason = d.blocked_reason.clone().unwrap_or_default();
        let title = format!("Dev, {} parou", d.title);
        let id = self.record(d, EventKind::NeedsInput, Priority::High, &title, Some(reason.clone()));
        let speech = crate::toast::speech(SpeechKind::Alert, title.clone());
        notify_delegation(&self.app, &Toast { title, body: reason, priority: Priority::High, event_id: id, speech }, &d.id, false);
    }

    fn failed(&self, d: &Delegation, message: &str) {
        let title = format!("Dev, a etapa {} de {} falhou", d.current_step + 1, d.steps.len());
        let body: String = message.lines().next().unwrap_or("").chars().take(160).collect();
        let id = self.record(d, EventKind::Error, Priority::High, &title, Some(message.to_string()));
        let speech = crate::toast::speech(SpeechKind::Alert, title.clone());
        notify_delegation(&self.app, &Toast { title, body, priority: Priority::High, event_id: id, speech }, &d.id, false);
    }

    fn done(&self, d: &Delegation) {
        let title = "Delegação concluída".to_string();
        let body = format!("{}. {} etapas, US$ {:.2}. Merge ou manter a branch?", d.title, d.steps.len(), d.cost_usd);
        let id = self.record(d, EventKind::TaskDone, Priority::High, &title, Some(body.clone()));
        // Fim da delegação vem da aprovação do próprio usuário: só toast.
        notify_delegation(&self.app, &Toast { title, body, priority: Priority::High, event_id: id, speech: None }, &d.id, false);
    }
}

/// Toast de delegação. O id da delegação vai no id da ação (o Windows devolve só ele).
fn notify_delegation(app: &AppHandle, t: &Toast, delegation_id: &str, approve: bool) {
    crate::voice_tauri::say_toast(app, t);
    let settings = app.state::<AppState>().settings();
    let dnd = crate::toast::dnd_active(&settings, chrono::Local::now().time());
    if !crate::toast::should_show(t, dnd) {
        return;
    }
    let type_id = format!("deleg-{delegation_id}{}", if approve { "-approve" } else { "" });
    let mut actions = vec![];
    if approve {
        actions.push(Action::builder(format!("approve:{delegation_id}"), "Aprovar").build());
    }
    actions.push(Action::builder(format!("open:{delegation_id}"), "Ver detalhes").build());
    *app.state::<AppState>().last_toast_tab.lock().unwrap() = Some("delegate".into());
    let n = app.notification();
    let _ = n.register_action_types(vec![ActionType::builder(&type_id).actions(actions).build()]);
    if let Err(e) = n.builder().title(&t.title).body(&t.body).action_type_id(&type_id).show() {
        log::warn!("toast da delegação: {e}");
    }
}

/// Ações dos toasts: `approve:<id>` aprova; o resto abre o painel.
pub fn on_toast_action(app: &AppHandle, action_id: &str) {
    if let Some(id) = action_id.strip_prefix("approve:") {
        let id = id.to_string();
        let handle = app.clone();
        std::thread::spawn(move || {
            if let Err(e) = engine(&handle).approve(&id) {
                log::warn!("aprovar pelo toast: {e:#}");
            }
        });
    } else if action_id.starts_with("open:") {
        crate::open_tab(app, "delegate");
    } else {
        // "tap": o Windows não diz qual toast foi; usa a aba do último mostrado.
        let last = app.state::<AppState>().last_toast_tab.lock().unwrap().clone();
        match last {
            Some(tab) => crate::open_tab(app, &tab),
            None => crate::show_main(app),
        }
    }
}

// ---------- alvo das ações do chat ----------

struct AppTarget<'a>(&'a AppHandle);

impl ActionTarget for AppTarget<'_> {
    fn start_delegation(&self, draft: DelegationDraft) -> anyhow::Result<()> {
        let e = engine(self.0);
        let d = e.create(draft)?;
        e.start(&d.id)?;
        Ok(())
    }
    fn approve_step(&self, id: &str) -> anyhow::Result<()> {
        engine(self.0).approve(id).map(|_| ())
    }
    fn adjust_step(&self, id: &str, feedback: &str) -> anyhow::Result<()> {
        engine(self.0).adjust(id, feedback).map(|_| ())
    }
    fn install_hooks(&self, dry_run: bool) -> anyhow::Result<()> {
        crate::hookcfg::apply_at(&nyx_claude::hooks::default_settings_path(), &crate::hookcfg::hook_exe(), true, dry_run)
            .map(|_| ())
    }
    fn create_context(&self, ctx: ProjectContext) -> anyhow::Result<()> {
        let state = self.0.state::<AppState>();
        core::save_context(&state.db.lock().unwrap(), ctx, Utc::now())?;
        crate::emit_agents(self.0);
        Ok(())
    }
    fn delegation(&self, id: &str) -> anyhow::Result<Delegation> {
        engine(self.0).get(id)
    }
    fn context_exists(&self, id: &str) -> bool {
        let state = self.0.state::<AppState>();
        let found = state.db.lock().unwrap().get_context(id).ok().flatten().is_some();
        found
    }
    fn open_panel(&self, tab: Option<&str>) -> anyhow::Result<()> {
        match tab {
            Some(t) => crate::open_tab(self.0, t),
            None => crate::show_main(self.0),
        }
        Ok(())
    }
}

// ---------- commands: delegação ----------

#[tauri::command]
pub async fn list_delegations(
    app: AppHandle,
    context_id: Option<String>,
    include_finished: Option<bool>,
) -> CmdResult<Vec<Delegation>> {
    blocking(app, move |a| engine(a).list(context_id.as_deref(), include_finished.unwrap_or(false))).await
}

#[tauri::command]
pub async fn get_delegation(app: AppHandle, id: String) -> CmdResult<Delegation> {
    blocking(app, move |a| engine(a).get(&id)).await
}

#[tauri::command]
pub async fn create_delegation(app: AppHandle, draft: DelegationDraft) -> CmdResult<Delegation> {
    blocking(app, move |a| engine(a).create(draft)).await
}

#[tauri::command]
pub async fn plan_delegation(app: AppHandle, context_id: String, goal: String) -> CmdResult<Vec<String>> {
    blocking(app, move |a| {
        let ai = ai(a);
        if !ai.available() {
            anyhow::bail!("IA indisponível. Defina NYX_ANTHROPIC_API_KEY e reinicie a Nyx.");
        }
        let state = a.state::<AppState>();
        let ctx = state.db.lock().unwrap().get_context(&context_id)?.ok_or_else(|| anyhow::anyhow!("Contexto não encontrado."))?;
        let context = format!("{}\n{}", ctx.name, ctx.instructions);
        let (steps, cost) = ai.plan_steps(goal.trim(), &context)?;
        state.db.lock().unwrap().add_ai_cost("plan", cost, Utc::now())?;
        Ok(steps)
    })
    .await
}

#[tauri::command]
pub async fn start_delegation(app: AppHandle, id: String) -> CmdResult<Delegation> {
    blocking(app, move |a| engine(a).start(&id)).await
}

#[tauri::command]
pub async fn approve_step(app: AppHandle, id: String) -> CmdResult<Delegation> {
    blocking(app, move |a| engine(a).approve(&id)).await
}

#[tauri::command]
pub async fn adjust_step(app: AppHandle, id: String, feedback: String) -> CmdResult<Delegation> {
    blocking(app, move |a| engine(a).adjust(&id, &feedback)).await
}

#[tauri::command]
pub async fn cancel_delegation(app: AppHandle, id: String) -> CmdResult<Delegation> {
    blocking(app, move |a| engine(a).cancel(&id)).await
}

#[tauri::command]
pub async fn finish_delegation(app: AppHandle, id: String, merge: bool) -> CmdResult<Delegation> {
    blocking(app, move |a| engine(a).finish(&id, merge)).await
}

// ---------- commands: chat e IA ----------

#[tauri::command]
pub async fn get_chat(app: AppHandle, context_id: Option<String>, limit: u32) -> CmdResult<Vec<ChatMessage>> {
    blocking(app, move |a| {
        let state = a.state::<AppState>();
        let db = state.db.lock().unwrap();
        chat::history(db.conn(), context_id.as_deref(), limit.clamp(1, 500))
    })
    .await
}

fn snapshot_for_chat(a: &AppHandle, context_id: Option<&str>) -> anyhow::Result<String> {
    let snap = crate::snapshot(a)?;
    let delegations = engine(a).list(context_id, false)?;
    let state = a.state::<AppState>();
    let (events, ctx) = {
        let db = state.db.lock().unwrap();
        let ctx = match context_id {
            Some(id) => db.get_context(id)?,
            None => None,
        };
        (db.list_events(5, context_id)?, ctx)
    };
    Ok(chat::snapshot_text(&snap, &delegations, &events, ctx.as_ref()))
}

#[tauri::command]
pub async fn send_chat(app: AppHandle, context_id: Option<String>, text: String) -> CmdResult<ChatMessage> {
    blocking(app, move |a| {
        let snapshot = snapshot_for_chat(a, context_id.as_deref())?;
        let state = a.state::<AppState>();
        let ai = ai(a);
        let (user, nyx) = chat::send(&state.db, ai.as_ref(), context_id.as_deref(), &text, &snapshot, Utc::now())?;
        let _ = a.emit(EVT_CHAT, &user);
        let _ = a.emit(EVT_CHAT, &nyx);
        crate::voice_tauri::say(a, SpeechKind::Chat, &nyx.text);
        Ok(nyx)
    })
    .await
}

#[tauri::command]
pub async fn clear_chat(app: AppHandle, context_id: Option<String>) -> CmdResult<()> {
    blocking(app, move |a| {
        let state = a.state::<AppState>();
        let db = state.db.lock().unwrap();
        chat::clear(db.conn(), context_id.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn run_chat_action(app: AppHandle, message_id: i64, action_id: String) -> CmdResult<ChatMessage> {
    blocking(app, move |a| {
        let state = a.state::<AppState>();
        let r = chat::run_action(&state.db, message_id, &action_id, &AppTarget(a));
        // O estado da ação (done/failed) já foi gravado; avisa a UI nos dois casos.
        if let Ok(Some(m)) = chat::get(state.db.lock().unwrap().conn(), message_id) {
            let _ = a.emit(EVT_CHAT, &m);
        }
        r
    })
    .await
}

#[tauri::command]
pub fn get_ai_status(app: AppHandle) -> CmdResult<AiStatus> {
    let state = app.state::<AppState>();
    let week = state.db.lock().unwrap().ai_cost_since(Utc::now() - chrono::Duration::days(7)).map_err(err)?;
    Ok(AiStatus { key_present: nyx_ai::key_present(), model: state.ai.model(), week_cost_usd: week })
}
