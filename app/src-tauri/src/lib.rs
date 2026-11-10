//! Nyx core. Owner: agent A (core). Spec: docs/specs/FASE-0-MVP.md §5.
//!
//! Tudo por evento: hooks chegam pelo servidor local, o uso chega pelo watcher dos
//! transcripts, e a única tarefa periódica é a checagem de sessões ociosas (1/min).

pub mod ai;
pub mod chat;
mod commands;
pub mod delegation;
#[cfg(test)]
mod delegation_tests;
mod night2;
pub mod runner;
pub mod core;
pub mod db;
mod hookcfg;
pub mod job;
pub mod model;
mod server;
pub mod sessions;
pub mod toast;
mod logger;
mod tray;
pub mod usage;
pub mod voice;
mod voice_tauri;
pub mod watcher;

use crate::db::Db;
use crate::model::{NyxEvent, Settings, UsageRange};
use crate::toast::Toast;
use chrono::Utc;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::{Action, ActionType, NotificationExt};

pub const EVT_EVENT: &str = "nyx://event";
pub const EVT_USAGE: &str = "nyx://usage";
pub const EVT_AGENTS: &str = "nyx://agents";
/// Settings mudaram fora da tela de Config (ex.: "Mudo" na bandeja). Payload: Settings.
pub const EVT_SETTINGS: &str = "nyx://settings";
const TOAST_ACTION_TYPE: &str = "nyx-event";
const IDLE_CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    settings: Mutex<Settings>,
    pub data_dir: PathBuf,
    pub runtime_path: PathBuf,
    pub badge: AtomicBool,
    /// Geração de mostrar/esconder do painel (cancela destruições agendadas).
    hide_gen: AtomicU64,
    dnd_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    mute_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    /// Voz (TTS): fila e playback numa thread própria; toca com o painel fechado.
    pub voice: voice::Voice,
    /// Noite 2: motor de delegação e IA (opt-in).
    pub deleg: Arc<delegation::Engine>,
    pub ai: Arc<dyn ai::AiBackend>,
    /// Aba a abrir quando a janela recém-criada montar (emitida no 1º `get_snapshot`).
    pending_tab: Mutex<Option<String>>,
    /// Aba do último toast mostrado (o clique "tap" não diz qual toast foi).
    pub last_toast_tab: Mutex<Option<String>>,
}

pub const EVT_OPEN_TAB: &str = "nyx://open-tab";
pub const TABS: &[&str] = &["agents", "delegate", "usage", "events", "contexts", "settings", "chat"];

/// Abre o painel numa aba. Com a janela já existente, emite `nyx://open-tab` na hora;
/// se ela vai ser criada agora, guarda a aba e emite quando a UI pedir o snapshot.
pub fn open_tab(app: &AppHandle, tab: &str) {
    if !TABS.contains(&tab) {
        show_main(app);
        return;
    }
    let existed = app.get_webview_window(MAIN_LABEL).is_some();
    if !existed {
        *app.state::<AppState>().pending_tab.lock().unwrap() = Some(tab.to_string());
    }
    show_main(app);
    if existed {
        let _ = app.emit(EVT_OPEN_TAB, serde_json::json!({ "tab": tab }));
    }
}

/// Chamado no `get_snapshot`: entrega a aba pendente depois que a UI registrou os listeners.
pub fn flush_pending_tab(app: &AppHandle) {
    let Some(tab) = app.state::<AppState>().pending_tab.lock().unwrap().take() else { return };
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // a UI pede o snapshot ao montar; um respiro para os `listen` terminarem
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let _ = handle.emit(EVT_OPEN_TAB, serde_json::json!({ "tab": tab }));
    });
}

/// Snapshot completo (inclui as delegações ativas no status da Nyx).
pub fn snapshot(app: &AppHandle) -> anyhow::Result<model::Snapshot> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let mut snap = core::snapshot(&state.db.lock().unwrap(), &settings, Utc::now())?;
    let delegs = state.deleg.list(None, false)?;
    snap.active_delegations = delegs
        .iter()
        .filter(|d| {
            use model::DelegationStatus as S;
            matches!(d.status, S::Running | S::Checkpoint | S::Handoff | S::Blocked)
        })
        .count() as u64;
    let waiting = delegs.iter().any(|d| {
        matches!(d.status, model::DelegationStatus::Checkpoint | model::DelegationStatus::Blocked | model::DelegationStatus::Failed)
    });
    let running = delegs.iter().any(|d| matches!(d.status, model::DelegationStatus::Running | model::DelegationStatus::Handoff));
    if waiting {
        snap.status = model::NyxStatus::Alert;
    } else if running && snap.status == model::NyxStatus::Active {
        snap.status = model::NyxStatus::Analyzing;
    }
    Ok(snap)
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
}

// ---------- janela (sob demanda) ----------
//
// A janela `main` não existe no início: o WebView2 é quase todo o custo de RAM.
// Ela é criada ao abrir o painel e destruída depois de 60 s escondida.

pub const MAIN_LABEL: &str = "main";
const DESTROY_AFTER_HIDDEN: std::time::Duration = std::time::Duration::from_secs(60);

/// Abre (criando se preciso) e foca o painel. Pode ser chamado de qualquer thread.
pub fn show_main(app: &AppHandle) {
    // Qualquer abertura cancela uma destruição agendada.
    app.state::<AppState>().hide_gen.fetch_add(1, Ordering::SeqCst);
    tray::set_badge(app, false);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(MAIN_LABEL) {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }
        let built = tauri::WebviewWindowBuilder::new(&handle, MAIN_LABEL, tauri::WebviewUrl::App("index.html".into()))
            .title("Nyx")
            .inner_size(420.0, 640.0)
            .min_inner_size(380.0, 520.0)
            .resizable(true)
            .background_color(tauri::window::Color(0x0D, 0x0B, 0x12, 0xFF))
            .visible(true)
            .focused(true)
            .build();
        if let Err(e) = built {
            log::error!("janela: {e}");
        }
    });
}

/// Esconde o painel e agenda a destruição; uma abertura no meio cancela.
fn hide_main(window: &tauri::Window) {
    let _ = window.hide();
    let app = window.app_handle().clone();
    let my_gen = app.state::<AppState>().hide_gen.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(DESTROY_AFTER_HIDDEN).await;
        if app.state::<AppState>().hide_gen.load(Ordering::SeqCst) != my_gen {
            return; // reaberta (ou escondida de novo) nesse meio tempo
        }
        if let Some(w) = app.get_webview_window(MAIN_LABEL) {
            if !w.is_visible().unwrap_or(true) {
                log::info!("painel escondido há 60 s: destruindo a janela");
                let _ = w.destroy();
            }
        }
    });
}

// ---------- emissão para a UI ----------

pub fn emit_agents(app: &AppHandle) {
    let state = app.state::<AppState>();
    let agents = core::agents(&state.db.lock().unwrap(), Utc::now());
    match agents {
        Ok(a) => {
            let _ = app.emit(EVT_AGENTS, a);
        }
        Err(e) => log::warn!("agents: {e:#}"),
    }
}

fn emit_usage(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let week = core::usage_summary(&state.db.lock().unwrap(), UsageRange::Week, None, &settings, Utc::now());
    if let Ok(w) = week {
        let _ = app.emit(EVT_USAGE, w);
    }
}

fn emit_event(app: &AppHandle, ev: &NyxEvent) {
    let _ = app.emit(EVT_EVENT, ev);
}

// ---------- toasts ----------

fn notify(app: &AppHandle, t: &Toast) {
    // a fala segue as próprias regras (mudo, não perturbe, onde fala)
    voice_tauri::say_toast(app, t);
    let settings = app.state::<AppState>().settings();
    let dnd = toast::dnd_active(&settings, chrono::Local::now().time());
    if !toast::should_show(t, dnd) {
        if dnd && t.priority == model::Priority::Medium {
            tray::set_badge(app, true);
        }
        return;
    }
    *app.state::<AppState>().last_toast_tab.lock().unwrap() = None;
    let mut b = app.notification().builder().title(&t.title).body(&t.body).action_type_id(TOAST_ACTION_TYPE);
    if let Some(id) = t.event_id {
        b = b.extra("eventId", id);
    }
    if let Err(e) = b.show() {
        log::warn!("toast: {e}");
    }
}

fn setup_notifications(app: &AppHandle) {
    let n = app.notification();
    let _ = n.register_action_types(vec![ActionType::builder(TOAST_ACTION_TYPE)
        .actions(vec![Action::builder("open", "Ver detalhes").build()])
        .build()]);
    let handle = app.clone();
    // Clique no toast ("tap") ou "Ver detalhes" abre o painel; "approve:<id>" aprova a etapa.
    let _ = n.on_action(move |a| night2::on_toast_action(&handle, a.action_id()));
}

// ---------- settings ----------

pub fn toggle_dnd(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut s = state.settings();
    s.do_not_disturb = !s.do_not_disturb;
    if let Err(e) = apply_settings(app, &state, s) {
        log::warn!("dnd: {e:#}");
    }
}

/// Grava as settings e aplica os efeitos: item do menu, autostart (só quando o toggle
/// muda; nunca liga sozinho) e alertas de uso com o teto novo.
pub fn apply_settings(app: &AppHandle, state: &AppState, s: Settings) -> anyhow::Result<Settings> {
    let before = state.settings();
    let saved = core::save_settings(&state.db.lock().unwrap(), s, &state.data_dir)?;
    *state.settings.lock().unwrap() = saved.clone();

    if let Some(item) = state.dnd_item.lock().unwrap().as_ref() {
        let _ = item.set_checked(saved.do_not_disturb);
    }
    if let Some(item) = state.mute_item.lock().unwrap().as_ref() {
        let _ = item.set_checked(saved.voice_muted);
    }
    let dnd = toast::dnd_active(&saved, chrono::Local::now().time());
    if !dnd {
        tray::set_badge(app, false);
    }
    // calou agora (mudo, voz desligada ou não perturbe): corta a fala em andamento
    if saved.voice_muted || !saved.voice_enabled || dnd {
        state.voice.stop();
    }
    if saved != before {
        let _ = app.emit(EVT_SETTINGS, &saved);
    }
    if saved.autostart != before.autostart {
        let al = app.autolaunch();
        let r = if saved.autostart { al.enable() } else { al.disable() };
        if let Err(e) = r {
            log::warn!("autostart: {e}");
        }
    }
    if saved.weekly_ceiling_usd != before.weekly_ceiling_usd || saved.alert_thresholds != before.alert_thresholds {
        check_alerts(app);
        emit_usage(app);
    }
    Ok(saved)
}

fn check_alerts(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let fired = core::check_usage_alerts(&state.db.lock().unwrap(), &settings, Utc::now());
    match fired {
        Ok(list) => {
            for (ev, t) in list {
                emit_event(app, &ev);
                notify(app, &t);
            }
        }
        Err(e) => log::warn!("alertas de uso: {e:#}"),
    }
}

// ---------- hooks ----------

fn handle_hook(app: &AppHandle, env: nyx_claude::HookEnvelope) {
    if env.source != "claude-code" {
        log::info!("evento de fonte desconhecida ignorado: {}", env.source);
        return;
    }
    let ne = nyx_claude::normalize_hook(&env.hook, &env.payload);
    // Hooks do `claude -p` das delegações: a delegação já avisa por conta própria.
    if ne.cwd.as_deref().map(sessions::normalize_path).is_some_and(|c| c.contains("/.nyx/worktrees/")) {
        log::debug!("hook de delegação ignorado ({})", env.hook);
        return;
    }
    let state = app.state::<AppState>();
    let applied = core::apply_event(&state.db.lock().unwrap(), &ne, Utc::now(), state.settings().long_task_minutes);
    match applied {
        Ok(a) => {
            if let Some(ev) = &a.event {
                emit_event(app, ev);
            }
            if a.session_changed {
                emit_agents(app);
            }
            if let Some(t) = &a.toast {
                notify(app, t);
            }
        }
        Err(e) => log::warn!("evento {}: {e:#}", env.hook),
    }
}

fn start_server(app: &AppHandle, runtime_path: PathBuf) -> anyhow::Result<()> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<nyx_claude::HookEnvelope>(1024);
    let listener = server::bind()?;
    let port = listener.local_addr()?.port();
    let token = server::new_token();
    server::write_runtime(
        &runtime_path,
        &nyx_claude::RuntimeInfo { port, token: token.clone(), pid: std::process::id() },
    )?;
    let router = server::router(token, tx);
    log::info!("servidor de eventos em 127.0.0.1:{port}");
    tauri::async_runtime::spawn(async move {
        if let Err(e) = server::serve(listener, router).await {
            log::error!("servidor de eventos: {e}");
        }
    });
    let handle = app.clone();
    std::thread::Builder::new().name("nyx-events".into()).spawn(move || {
        while let Ok(env) = rx.recv() {
            handle_hook(&handle, env);
        }
    })?;
    Ok(())
}

fn start_idle_check(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut iv = tokio::time::interval(IDLE_CHECK_EVERY);
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            iv.tick().await;
            let n = {
                let state = handle.state::<AppState>();
                let r = core::sweep_idle(&state.db.lock().unwrap(), Utc::now());
                r.unwrap_or(0)
            };
            if n > 0 {
                emit_agents(&handle);
            }
        }
    });
}

fn start_watcher(app: &AppHandle) {
    let state = app.state::<AppState>();
    let handle = app.clone();
    let r = watcher::spawn(
        state.db.clone(),
        nyx_claude::projects_dir(),
        nyx_claude::read_from,
        nyx_claude::estimate_cost_usd,
        move || {
            emit_usage(&handle);
            emit_agents(&handle);
            check_alerts(&handle);
        },
    );
    if let Err(e) = r {
        log::error!("watcher: {e}");
    }
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = nyx_claude::data_dir();
    std::fs::create_dir_all(&data_dir)?;
    logger::init(&data_dir.join("logs").join("nyx.log"));
    log::info!("Nyx iniciando (pid {}, dados em {})", std::process::id(), data_dir.display());
    match job::init() {
        job::JobMode::WholeApp => log::info!("job object: o Nyx e todos os filhos (claude, WebView2) saem juntos"),
        job::JobMode::PerChild => {
            log::warn!("job object: o Windows recusou o Nyx no job; cada claude entra após o spawn (fallback)")
        }
        job::JobMode::Unavailable => {
            log::warn!("job object indisponível: processos das delegações podem sobreviver a um crash")
        }
    }
    let db = Db::open(&data_dir.join("nyx.db"))?;
    let settings = core::load_settings(
        &db,
        || nyx_claude::default_weekly_ceiling_usd(model::PlanPreset::Max5x.as_str()),
        &data_dir,
    )?;
    let runtime_path = nyx_claude::runtime_file();
    let dnd = settings.do_not_disturb;
    let muted = settings.voice_muted;
    let db = Arc::new(Mutex::new(db));
    let ai: Arc<dyn ai::AiBackend> = Arc::new(ai::NyxAi::from_env());
    log::info!("IA: {}", if ai.available() { "chave presente" } else { "sem NYX_ANTHROPIC_API_KEY (indisponível)" });
    let deleg = delegation::Engine::new(
        db.clone(),
        Arc::new(runner::ProcessRunner::default()),
        ai.clone(),
        Arc::new(night2::TauriSink { app: app.handle().clone() }),
        Box::new(runner::claude_exe),
    );
    match deleg.recover() {
        Ok(0) => {}
        Ok(n) => log::info!("{n} delegação(ões) interrompida(s) marcadas como falhas"),
        Err(e) => log::warn!("recuperação das delegações: {e:#}"),
    }
    let voice = voice_tauri::spawn(app.handle(), &data_dir)?;
    app.manage(AppState {
        voice,
        mute_item: Mutex::new(None),
        deleg,
        ai,
        pending_tab: Mutex::new(None),
        last_toast_tab: Mutex::new(None),
        db,
        settings: Mutex::new(settings),
        data_dir,
        runtime_path: runtime_path.clone(),
        badge: AtomicBool::new(false),
        hide_gen: AtomicU64::new(0),
        dnd_item: Mutex::new(None),
    });

    let handle = app.handle().clone();
    let (dnd_item, mute_item) = tray::build(&handle, dnd, muted)?;
    *app.state::<AppState>().dnd_item.lock().unwrap() = Some(dnd_item);
    *app.state::<AppState>().mute_item.lock().unwrap() = Some(mute_item);
    setup_notifications(&handle);
    start_server(&handle, runtime_path).map_err(|e| e.to_string())?;
    start_watcher(&handle);
    start_idle_check(&handle);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // Precisa ser o primeiro plugin: a segunda instância só abre o painel da primeira.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Só registra o plugin; liga/desliga apenas pelo toggle das settings (padrão: desligado).
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(setup)
        .on_window_event(|window, event| {
            // Fechar a janela só esconde: a Nyx continua na bandeja.
            if window.label() != MAIN_LABEL {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                hide_main(window);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_events,
            commands::get_usage,
            commands::list_contexts,
            commands::save_context,
            commands::delete_context,
            commands::get_settings,
            commands::save_settings,
            commands::export_usage_csv,
            commands::mark_event_read,
            commands::get_hooks_status,
            commands::install_hooks,
            commands::uninstall_hooks,
            night2::list_delegations,
            night2::get_delegation,
            night2::create_delegation,
            night2::plan_delegation,
            night2::start_delegation,
            night2::approve_step,
            night2::adjust_step,
            night2::cancel_delegation,
            night2::finish_delegation,
            night2::get_chat,
            night2::send_chat,
            night2::clear_chat,
            night2::run_chat_action,
            night2::get_ai_status,
            voice_tauri::get_voice_status,
            voice_tauri::test_voice,
            voice_tauri::set_voice_muted,
            voice_tauri::stop_speech,
            voice_tauri::speak_opening,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| match event {
        // Sem janelas (painel destruído) o Tauri pede para sair com code None: a Nyx
        // continua na bandeja. Só o "Sair" (app.exit(0), code Some) encerra.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<AppState>() {
                server::remove_runtime(&state.runtime_path);
                let n = state.deleg.kill_all();
                if n > 0 {
                    log::info!("{n} processo(s) do Claude Code encerrados na saída");
                }
            }
            log::info!("Nyx encerrada");
            log::logger().flush();
        }
        _ => {}
    });
}
