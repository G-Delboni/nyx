//! Claude Code adapter for Nyx. Contract: docs/specs/FASE-0-MVP.md §2–§3.
//! Owner: agent B (adapters). The public signatures below are the contract the
//! core (agent A) compiles against; do not change them without the orchestrator.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub mod delegate;
pub mod hooks;
pub mod settings_merge;

#[cfg(test)]
mod tests;

pub const PARSER_VERSION: u32 = 1;

// ---------- §2 hook events ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    TaskDone,
    NeedsInput,
    Error,
    SessionStart,
    SessionEnd,
    PromptSubmitted,
    ToolUse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedEvent {
    pub kind: EventKind,
    pub session_id: String,
    pub cwd: Option<String>,
    pub transcript_path: Option<String>,
    pub title: String,
    pub detail: Option<String>,
    pub raw_hook: String,
}

/// Body that `nyx-hook` POSTs to `/v1/events`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEnvelope {
    pub source: String, // "claude-code"
    pub hook: String,   // "Stop", "Notification", ...
    pub received_at: String,
    pub payload: serde_json::Value,
}

/// Content of `%APPDATA%\Nyx\runtime.json` (or `$NYX_DATA_DIR\runtime.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// Data dir: `$NYX_DATA_DIR` if set, else `%APPDATA%\Nyx`.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("NYX_DATA_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("Nyx")
}

pub fn runtime_file() -> PathBuf {
    data_dir().join("runtime.json")
}

/// Last path component of a Windows or POSIX path (`C:\x\projeto demo` -> `projeto demo`).
pub fn folder_name(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_string()
}

fn str_field(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Turns a Claude Code hook payload into a Nyx event. Never panics: unknown hooks
/// become a neutral `ToolUse` activity event. Titles follow docs/PERSONAGEM.md §2
/// (dry, no exclamation marks, no emoji).
pub fn normalize_hook(hook: &str, payload: &serde_json::Value) -> NormalizedEvent {
    // The CLI argument wins; fall back to the name the payload carries.
    let hook_name = if hook.is_empty() {
        str_field(payload, "hook_event_name").unwrap_or_default()
    } else {
        hook.to_string()
    };
    let cwd = str_field(payload, "cwd");
    let place = cwd
        .as_deref()
        .map(folder_name)
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "um projeto".to_string());
    let mut detail = None;

    let (kind, title) = match hook_name.as_str() {
        "Stop" => {
            detail = str_field(payload, "last_assistant_message").map(|m| truncate_chars(&m, 280));
            (EventKind::TaskDone, "Claude Code concluiu".to_string())
        }
        // A subagent finishing is activity, not the session finishing: it must not
        // flip the session to "done".
        "SubagentStop" => (EventKind::ToolUse, "Subagente concluiu".to_string()),
        "Notification" => {
            detail = str_field(payload, "message");
            let ntype = str_field(payload, "notification_type").unwrap_or_default();
            match ntype.as_str() {
                "auth_success"
                | "plugin_notification"
                | "elicitation_complete"
                | "elicitation_response"
                | "quota_auto_resume_fired"
                | "quota_auto_resume_stale"
                | "quota_auto_resume_disabled" => (EventKind::ToolUse, "Claude Code avisou".to_string()),
                "auth_storage_failure" => (
                    EventKind::Error,
                    "Claude Code falhou ao guardar a autenticação".to_string(),
                ),
                _ => (
                    EventKind::NeedsInput,
                    format!("Dev, o Claude Code em {place} quer uma resposta sua."),
                ),
            }
        }
        "PreToolUse" => {
            detail = str_field(payload, "tool_name");
            (EventKind::ToolUse, "Claude Code em execução".to_string())
        }
        "SessionStart" => {
            detail = str_field(payload, "source");
            (EventKind::SessionStart, "Sessão do Claude Code iniciada".to_string())
        }
        "SessionEnd" => {
            detail = str_field(payload, "reason");
            (EventKind::SessionEnd, "Sessão do Claude Code encerrada".to_string())
        }
        "UserPromptSubmit" => (EventKind::PromptSubmitted, "Prompt enviado ao Claude Code".to_string()),
        other => (EventKind::ToolUse, format!("Evento do Claude Code: {other}")),
    };

    NormalizedEvent {
        kind,
        session_id: str_field(payload, "session_id").unwrap_or_default(),
        cwd,
        transcript_path: str_field(payload, "transcript_path"),
        title,
        detail,
        raw_hook: hook_name,
    }
}

// ---------- §3 usage ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub message_id: String,
    pub session_id: String,
    pub cwd: Option<String>,
    pub model: String,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

pub fn parse_line(line: &str) -> Option<UsageRecord> {
    let line = line.trim_start_matches('\u{feff}').trim();
    // Cheap pre-filter: most transcript lines are not assistant messages with usage.
    if line.is_empty() || !line.contains("\"usage\"") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let msg = v.get("message")?;
    let usage = msg.get("usage")?.as_object()?;
    let message_id = msg.get("id")?.as_str()?.to_string();
    let model = msg.get("model")?.as_str()?.to_string();
    if model == "<synthetic>" {
        return None;
    }
    let ts = chrono::DateTime::parse_from_rfc3339(v.get("timestamp")?.as_str()?)
        .ok()?
        .with_timezone(&chrono::Utc);
    let n = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    Some(UsageRecord {
        message_id,
        session_id: v
            .get("sessionId")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string(),
        cwd: v.get("cwd").and_then(|x| x.as_str()).map(str::to_string),
        model,
        ts,
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_write: n("cache_creation_input_tokens"),
        cache_read: n("cache_read_input_tokens"),
    })
}

/// Reads from `offset` (bytes); returns records and the new offset (complete lines only).
/// Dedupes by `message.id` within the call (streamed messages repeat; the line with the
/// largest `output` wins). If the file shrank below `offset`, it restarts at 0.
pub fn read_from(path: &Path, offset: u64) -> std::io::Result<(Vec<UsageRecord>, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = if offset > len { 0 } else { offset };
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let complete = match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => i + 1,
        None => return Ok((Vec::new(), start)),
    };
    let mut out: Vec<UsageRecord> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for raw in buf[..complete].split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(raw);
        if let Some(r) = parse_line(&line) {
            match index.get(&r.message_id) {
                Some(&i) => {
                    if r.output >= out[i].output {
                        out[i] = r;
                    }
                }
                None => {
                    index.insert(r.message_id.clone(), out.len());
                    out.push(r);
                }
            }
        }
    }
    Ok((out, start + complete as u64))
}

// ---------- cost ----------

/// Price in USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64, // 5-minute TTL (1.25x input)
    pub cache_read: f64,
}

pub const PRICE_TABLE_VERSION: &str = "2026-11-07";

const fn p(input: f64, output: f64, cache_write: f64, cache_read: f64) -> Price {
    Price { input, output, cache_write, cache_read }
}

/// Price lookup by model family. Source: Anthropic public API pricing as listed in the
/// `claude-api` skill (model table cached 2026-10-06), consulted 2026-11-07. Cache write
/// is the 5-minute rate (1.25x input); transcripts also carry 1-hour writes (2x input),
/// which `UsageRecord` does not split, so those are slightly underestimated. Older
/// families (Opus 4.x/3, Sonnet 4.x/3.7, Haiku 4.5/3.x) come from public pricing as
/// remembered, not re-verified. Returns `(price, known)`; unknown models fall back to
/// Sonnet 5.5 pricing with `known = false`.
pub fn price_for(model: &str) -> (Price, bool) {
    let m = model.to_ascii_lowercase();
    let has = |s: &str| m.contains(s);
    let price = if has("fable") || has("mythos") {
        if has("5-1") {
            p(10.0, 50.0, 12.5, 0.25)
        } else {
            p(10.0, 50.0, 12.5, 1.0)
        }
    } else if has("opus") {
        if has("opus-5-5") {
            p(4.0, 20.0, 5.0, 0.20)
        } else if has("opus-5")
            || has("opus-4-8")
            || has("opus-4-7")
            || has("opus-4-6")
            || has("opus-4-5")
        {
            p(5.0, 25.0, 6.25, 0.50)
        } else {
            // Opus 4.1, Opus 4, Opus 3
            p(15.0, 75.0, 18.75, 1.50)
        }
    } else if has("sonnet") {
        if has("sonnet-5") {
            p(2.0, 10.0, 2.5, 0.20)
        } else {
            p(3.0, 15.0, 3.75, 0.30)
        }
    } else if has("haiku") {
        if has("haiku-5") {
            p(0.10, 0.50, 0.125, 0.01)
        } else if has("haiku-4") {
            p(1.0, 5.0, 1.25, 0.10)
        } else if has("3-5-haiku") || has("haiku-3-5") {
            p(0.80, 4.0, 1.0, 0.08)
        } else {
            p(0.25, 1.25, 0.30, 0.03)
        }
    } else {
        return (p(2.0, 10.0, 2.5, 0.20), false);
    };
    (price, true)
}

fn warn_unknown_model(model: &str) {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let set = SEEN.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    if let Ok(mut s) = set.lock() {
        if s.insert(model.to_string()) {
            eprintln!("[nyx-claude] unknown model '{model}': priced as Sonnet 5.5");
        }
    }
}

pub fn estimate_cost_usd(r: &UsageRecord) -> f64 {
    let (pr, known) = price_for(&r.model);
    if !known {
        warn_unknown_model(&r.model);
    }
    (r.input as f64 * pr.input
        + r.output as f64 * pr.output
        + r.cache_write as f64 * pr.cache_write
        + r.cache_read as f64 * pr.cache_read)
        / 1_000_000.0
}

/// `~/.claude/projects`
pub fn projects_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("projects")
}

/// Suggested initial weekly ceiling (estimated USD) per plan preset: "pro" | "max5x" | "max20x".
/// Provisional: plans publish no token ceilings. See docs/specs/NOTAS-B.md for the basis.
pub fn default_weekly_ceiling_usd(preset: &str) -> f64 {
    match preset {
        "pro" => CEILING_PRO,
        "max20x" => CEILING_MAX20X,
        _ => CEILING_MAX5X,
    }
}
// Calibrated 2026-11-07: the app's usage card showed 64% of the Max weekly limit
// while the local transcripts summed ~US$ 736 over the same 7 days -> ~US$ 1150.
const CEILING_PRO: f64 = 230.0; // max5x / 5 (plan multiplier)
const CEILING_MAX5X: f64 = 1150.0;
const CEILING_MAX20X: f64 = 4600.0; // max5x * 4 (plan multiplier)
