//! nyx-hook — contract: docs/specs/FASE-0-MVP.md §2. Owner: agent B (adapters).
//!
//! `nyx-hook event <Hook>`   forwards a Claude Code hook payload (stdin) to the Nyx core.
//!                           Always exits 0 and never writes to stdout.
//! `nyx-hook install|uninstall [--settings <path>] [--dry-run]`   edits settings.json.

mod installer;

use nyx_claude::{HookEnvelope, RuntimeInfo};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const TOTAL_BUDGET: Duration = Duration::from_millis(1500);
const MAX_STDIN: u64 = 1024 * 1024;
const LOG_MAX_BYTES: u64 = 1024 * 1024;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("event") => {
            // Never let anything escape: the hook must stay silent and exit 0.
            let hook = args.get(1).cloned().unwrap_or_default();
            let _ = std::panic::catch_unwind(|| run_event(&hook));
            std::process::exit(0);
        }
        Some("install") => std::process::exit(installer::run(&args[1..], true)),
        Some("uninstall") => std::process::exit(installer::run(&args[1..], false)),
        _ => {
            eprintln!(
                "uso: nyx-hook event <Hook> | install [--settings <path>] [--dry-run] | uninstall [--settings <path>] [--dry-run]"
            );
            std::process::exit(2);
        }
    }
}

fn run_event(hook: &str) {
    let started = Instant::now();
    let mut raw = String::new();
    let _ = std::io::stdin().take(MAX_STDIN).read_to_string(&mut raw);
    let payload: serde_json::Value =
        serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap_or(serde_json::Value::Null);

    let runtime: RuntimeInfo = match std::fs::read_to_string(nyx_claude::runtime_file())
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
    {
        Some(r) => r,
        None => {
            log(&format!("{hook}: nyx not running (no runtime.json)"));
            return;
        }
    };

    let envelope = HookEnvelope {
        source: "claude-code".to_string(),
        hook: hook.to_string(),
        received_at: chrono::Utc::now().to_rfc3339(),
        payload,
    };
    let remaining = TOTAL_BUDGET.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        log(&format!("{hook}: budget exhausted before POST"));
        return;
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(remaining))
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("http://127.0.0.1:{}/v1/events", runtime.port);
    match agent
        .post(&url)
        .header("X-Nyx-Token", &runtime.token)
        .send_json(&envelope)
    {
        Ok(resp) => log(&format!(
            "{hook}: {} in {}ms",
            resp.status().as_u16(),
            started.elapsed().as_millis()
        )),
        Err(e) => log(&format!("{hook}: send failed: {e}")),
    }
}

/// Best-effort log to `data_dir()/logs/hook.log`, rotated to `hook.log.1` at 1 MB.
fn log(msg: &str) {
    let dir = nyx_claude::data_dir().join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("hook.log");
    if std::fs::metadata(&path).map_or(false, |m| m.len() >= LOG_MAX_BYTES) {
        let _ = std::fs::rename(&path, dir.join("hook.log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {}", chrono::Utc::now().to_rfc3339(), msg);
    }
}
