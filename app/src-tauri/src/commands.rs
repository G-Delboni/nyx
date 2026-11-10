//! Commands do Tauri (contrato em `app/src/api/contract.ts`). O JS manda os args em
//! camelCase e o Tauri converte para os parâmetros snake_case daqui.

use crate::model::*;
use crate::{core, emit_agents, AppState};
use chrono::Utc;
use tauri::{AppHandle, State};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_snapshot(app: AppHandle) -> CmdResult<Snapshot> {
    let snap = crate::snapshot(&app).map_err(err)?;
    crate::flush_pending_tab(&app);
    Ok(snap)
}

#[tauri::command]
pub fn get_events(state: State<'_, AppState>, limit: u32, context_id: Option<String>) -> CmdResult<Vec<NyxEvent>> {
    let db = state.db.lock().unwrap();
    db.list_events(limit.clamp(1, 1000), context_id.as_deref()).map_err(err)
}

#[tauri::command]
pub fn get_usage(state: State<'_, AppState>, range: UsageRange, context_id: Option<String>) -> CmdResult<UsageSummary> {
    let settings = state.settings();
    let db = state.db.lock().unwrap();
    core::usage_summary(&db, range, context_id.as_deref(), &settings, Utc::now()).map_err(err)
}

#[tauri::command]
pub fn list_contexts(state: State<'_, AppState>) -> CmdResult<Vec<ProjectContext>> {
    state.db.lock().unwrap().list_contexts().map_err(err)
}

#[tauri::command]
pub fn save_context(app: AppHandle, state: State<'_, AppState>, ctx: ProjectContext) -> CmdResult<ProjectContext> {
    let saved = {
        let db = state.db.lock().unwrap();
        core::save_context(&db, ctx, Utc::now()).map_err(err)?
    };
    emit_agents(&app); // o contexto das sessões pode ter mudado
    Ok(saved)
}

#[tauri::command]
pub fn delete_context(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.db.lock().unwrap().delete_context(&id).map_err(err)?;
    emit_agents(&app);
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings())
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    crate::apply_settings(&app, &state, settings).map_err(err)
}

#[tauri::command]
pub fn export_usage_csv(state: State<'_, AppState>, range: UsageRange, context_id: Option<String>) -> CmdResult<String> {
    let db = state.db.lock().unwrap();
    let path = core::export_csv(&db, range, context_id.as_deref(), Utc::now(), &core::export_dir()).map_err(err)?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn mark_event_read(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.db.lock().unwrap().mark_event_read(id).map_err(err)
}

// ---------- hooks do Claude Code (~/.claude/settings.json) ----------

#[tauri::command]
pub fn get_hooks_status() -> CmdResult<nyx_claude::hooks::HooksStatus> {
    crate::hookcfg::status_at(&nyx_claude::hooks::default_settings_path(), &crate::hookcfg::hook_exe()).map_err(err)
}

#[tauri::command]
pub fn install_hooks(dry_run: bool) -> CmdResult<nyx_claude::hooks::HooksReport> {
    crate::hookcfg::apply_at(&nyx_claude::hooks::default_settings_path(), &crate::hookcfg::hook_exe(), true, dry_run)
        .map_err(err)
}

#[tauri::command]
pub fn uninstall_hooks(dry_run: bool) -> CmdResult<nyx_claude::hooks::HooksReport> {
    crate::hookcfg::apply_at(&nyx_claude::hooks::default_settings_path(), &crate::hookcfg::hook_exe(), false, dry_run)
        .map_err(err)
}
