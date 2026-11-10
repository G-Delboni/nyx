//! Install/uninstall of the Nyx hooks in a Claude Code `settings.json`, with status
//! and a short diff. Used by the `nyx-hook` CLI and by the core (panel onboarding).
//! Contract: `app/src/api/contract.ts` (HooksStatus, HooksReport).

use crate::settings_merge::{self, NYX_HOOK_EVENTS};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

const MAX_DIFF_LINES: usize = 15;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksStatus {
    pub settings_path: PathBuf,
    pub hook_exe: PathBuf,
    pub hook_exe_exists: bool,
    pub installed: bool,
    pub missing_events: Vec<String>,
    pub other_hooks: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksReport {
    pub dry_run: bool,
    pub changed: bool,
    pub backup_path: Option<PathBuf>,
    pub diff: String,
    pub status: HooksStatus,
}

/// `~/.claude/settings.json`
pub fn default_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
}

/// `nyx-hook.exe` next to the running executable (where the app looks for it).
pub fn sibling_hook_exe() -> PathBuf {
    let name = if cfg!(windows) { "nyx-hook.exe" } else { "nyx-hook" };
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    clean_path(&dir.join(name))
}

/// Drops the Windows verbatim prefix (`\\?\`) that `current_exe` may carry.
pub fn clean_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

/// Reads settings; `Ok(None)` when the file does not exist. Invalid JSON is an error
/// (the file is never overwritten in that case).
fn read_settings(path: &Path) -> Result<Option<Value>> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let text = text.trim_start_matches('\u{feff}');
            if text.trim().is_empty() {
                return Ok(Some(serde_json::json!({})));
            }
            let v = serde_json::from_str(text).map_err(|e| {
                anyhow!("{} não é um JSON válido ({e}); nada foi alterado", path.display())
            })?;
            Ok(Some(v))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("não consegui ler {}", path.display())),
    }
}

/// All (event, command) hook entries in `settings`, split into Nyx's and the rest.
fn entries(settings: &Value) -> (Vec<(String, String)>, usize) {
    let mut nyx = Vec::new();
    let mut other = 0;
    if let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) {
        for (event, groups) in hooks {
            for group in groups.as_array().into_iter().flatten() {
                for h in group.get("hooks").and_then(|l| l.as_array()).into_iter().flatten() {
                    match h.get("command").and_then(|c| c.as_str()) {
                        Some(c) if settings_merge::is_nyx_command(c) => nyx.push((event.clone(), c.to_string())),
                        _ => other += 1,
                    }
                }
            }
        }
    }
    (nyx, other)
}

fn status_of(settings: &Value, settings_path: &Path, hook_exe: &Path) -> HooksStatus {
    let exe = hook_exe.to_string_lossy();
    let (nyx, other_hooks) = entries(settings);
    let missing_events: Vec<String> = NYX_HOOK_EVENTS
        .iter()
        .filter(|ev| {
            let want = settings_merge::command_for(&exe, ev);
            !nyx.iter().any(|(e, c)| e == *ev && *c == want)
        })
        .map(|s| s.to_string())
        .collect();
    HooksStatus {
        settings_path: settings_path.to_path_buf(),
        hook_exe: hook_exe.to_path_buf(),
        hook_exe_exists: hook_exe.is_file(),
        installed: missing_events.is_empty(),
        missing_events,
        other_hooks,
    }
}

/// Where the Nyx hooks stand in `settings` (a missing file means nothing installed).
pub fn status(settings: &Path, hook_exe: &Path) -> Result<HooksStatus> {
    let value = read_settings(settings)?.unwrap_or_else(|| serde_json::json!({}));
    Ok(status_of(&value, settings, hook_exe))
}

fn diff_of(before: &Value, after: &Value) -> String {
    let (b, _) = entries(before);
    let (a, _) = entries(after);
    let mut lines: Vec<String> = Vec::new();
    for (ev, c) in &b {
        if !a.contains(&(ev.clone(), c.clone())) {
            lines.push(format!("- {ev}: {c}"));
        }
    }
    for (ev, c) in &a {
        if !b.contains(&(ev.clone(), c.clone())) {
            lines.push(format!("+ {ev}: {c}"));
        }
    }
    if lines.len() > MAX_DIFF_LINES {
        let extra = lines.len() - (MAX_DIFF_LINES - 1);
        lines.truncate(MAX_DIFF_LINES - 1);
        lines.push(format!("... e mais {extra} linhas"));
    }
    lines.join("\n")
}

/// Installs or removes the Nyx hooks. With `dry_run` nothing is written. Otherwise:
/// backup `<name>.nyx-backup-<timestamp>` (when the file exists), write to a temp file
/// and rename over the target. Existing entries and key order are preserved.
pub fn apply(settings: &Path, hook_exe: &Path, install: bool, dry_run: bool) -> Result<HooksReport> {
    let existing = read_settings(settings)?;
    let file_exists = existing.is_some();
    let current = existing.unwrap_or_else(|| serde_json::json!({}));
    let updated = if install {
        settings_merge::install(current.clone(), &hook_exe.to_string_lossy())
    } else {
        settings_merge::uninstall(current.clone())
    };
    let changed = updated != current;
    let diff = diff_of(&current, &updated);
    let status = status_of(&updated, settings, hook_exe);
    let mut report = HooksReport { dry_run, changed, backup_path: None, diff, status };
    if !changed || dry_run {
        return Ok(report);
    }

    let text = serde_json::to_string_pretty(&updated)? + "\n";
    if file_exists {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let name = settings.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let backup = settings.with_file_name(format!("{name}.nyx-backup-{stamp}"));
        std::fs::copy(settings, &backup)
            .with_context(|| format!("falha ao criar o backup {}; nada foi alterado", backup.display()))?;
        report.backup_path = Some(backup);
    } else if let Some(dir) = settings.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = settings.with_extension("json.nyx-tmp");
    std::fs::write(&tmp, text).with_context(|| format!("falha ao escrever {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, settings) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("falha ao substituir {}", settings.display()));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ORCA: &str = "C:/Users/dev/.orca/agent-hooks/orca-agent-hook.exe claude";

    fn sample() -> Value {
        json!({
            "model": "opus",
            "hooks": {
                "Stop": [{"hooks":[{"type":"command","command":ORCA,"timeout":10}]}],
                "PermissionRequest": [{"matcher":"*","hooks":[{"type":"command","command":ORCA,"timeout":10}]}]
            }
        })
    }

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("settings.json");
        std::fs::write(&f, serde_json::to_string_pretty(&sample()).unwrap()).unwrap();
        let exe = d.path().join("nyx-hook.exe");
        std::fs::write(&exe, b"x").unwrap();
        (d, f, exe)
    }

    #[test]
    fn status_missing_file_is_not_installed() {
        let d = tempfile::tempdir().unwrap();
        let s = status(&d.path().join("nope.json"), &d.path().join("nyx-hook.exe")).unwrap();
        assert!(!s.installed && !s.hook_exe_exists);
        assert_eq!(s.missing_events.len(), 7);
        assert_eq!(s.other_hooks, 0);
    }

    #[test]
    fn status_counts_other_hooks() {
        let (_d, f, exe) = setup();
        let s = status(&f, &exe).unwrap();
        assert!(!s.installed && s.hook_exe_exists);
        assert_eq!(s.other_hooks, 2);
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let (d, f, exe) = setup();
        let before = std::fs::read_to_string(&f).unwrap();
        let r = apply(&f, &exe, true, true).unwrap();
        assert!(r.dry_run && r.changed && r.backup_path.is_none());
        assert!(r.status.installed, "status as it would be");
        assert_eq!(r.diff.lines().count(), 7);
        assert!(r.diff.lines().all(|l| l.starts_with("+ ")));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), before);
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2);
    }

    #[test]
    fn install_uninstall_roundtrip() {
        let (_d, f, exe) = setup();
        let r = apply(&f, &exe, true, false).unwrap();
        assert!(r.changed && r.status.installed);
        let backup = r.backup_path.unwrap();
        assert!(backup.exists());
        assert!(status(&f, &exe).unwrap().installed);
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(written.as_object().unwrap().keys().collect::<Vec<_>>(), ["model", "hooks"]);
        assert_eq!(written["hooks"]["PermissionRequest"], sample()["hooks"]["PermissionRequest"]);

        // idempotent: no change, no new backup
        let again = apply(&f, &exe, true, false).unwrap();
        assert!(!again.changed && again.backup_path.is_none() && again.diff.is_empty());

        let un = apply(&f, &exe, false, false).unwrap();
        assert!(un.changed && !un.status.installed);
        assert!(un.diff.lines().all(|l| l.starts_with("- ")));
        let restored: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(restored, sample());
    }

    #[test]
    fn moved_exe_shows_missing_then_refreshes() {
        let (d, f, exe) = setup();
        apply(&f, &exe, true, false).unwrap();
        let other = d.path().join("sub").join("nyx-hook.exe");
        let s = status(&f, &other).unwrap();
        assert!(!s.installed && s.missing_events.len() == 7);
        let r = apply(&f, &other, true, false).unwrap();
        assert!(r.changed && r.status.installed);
        assert!(r.diff.lines().count() <= MAX_DIFF_LINES);
    }

    #[test]
    fn invalid_json_is_an_error_and_untouched() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("settings.json");
        std::fs::write(&f, "{ nope").unwrap();
        assert!(apply(&f, &d.path().join("nyx-hook.exe"), true, false).is_err());
        assert!(status(&f, &d.path().join("nyx-hook.exe")).is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{ nope");
    }

    #[test]
    fn creates_missing_file_and_serializes_camel_case() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("new").join("settings.json");
        let r = apply(&f, &d.path().join("nyx-hook.exe"), true, false).unwrap();
        assert!(f.exists() && r.backup_path.is_none());
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["dryRun"], false);
        assert!(v["backupPath"].is_null());
        assert!(v["status"]["settingsPath"].is_string());
        assert!(v["status"]["missingEvents"].is_array());
        assert!(v["status"]["hookExeExists"].is_boolean());
    }

    #[test]
    fn sibling_exe_is_named_nyx_hook() {
        let p = sibling_hook_exe();
        assert!(p.file_name().unwrap().to_string_lossy().starts_with("nyx-hook"));
        assert!(!p.to_string_lossy().starts_with(r"\\?\"));
    }
}
