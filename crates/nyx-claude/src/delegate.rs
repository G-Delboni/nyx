//! Phase 2 groundwork: build the `claude -p` command for one delegated step and parse
//! its `stream-json` output. Pure library code: nothing here spawns Claude.
//!
//! Flags per https://code.claude.com/docs/en/headless (checked 2026-11-07):
//! `-p <prompt> --output-format stream-json --verbose [--resume <id>] [--model m]
//! [--permission-mode mode]`. The stream is one JSON object per line: `system`
//! (subtype `init`, carries `session_id`), `assistant`, `user` (tool results) and a
//! final `result` (`subtype`, `is_error`, `result`, `session_id`, `num_turns`,
//! `duration_ms`, `total_cost_usd`, `usage`). Event shapes are not a stable API, so
//! parsing ignores whatever it does not recognise.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::BufRead;
use std::path::{Path, PathBuf};

pub const MAX_REPORT_LINES: usize = 5;
const MAX_LINE_CHARS: usize = 300;
const MAX_SUMMARY_CHARS: usize = 80;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepRequest {
    pub cwd: PathBuf,
    pub prompt: String,
    pub context_instructions: Option<String>,
    pub references: Vec<String>,
    pub resume_session: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
}

/// Prompt actually sent: a short context block on top, the step, and the checkpoint
/// report request at the end (CLAUDE.md: title + at most 5 lines).
pub fn build_prompt(req: &StepRequest) -> String {
    let mut out = String::new();
    let instructions = req.context_instructions.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if instructions.is_some() || !req.references.is_empty() {
        out.push_str("[Contexto da Nyx]\n");
        if let Some(i) = instructions {
            out.push_str("Instruções permanentes:\n");
            out.push_str(i);
            out.push('\n');
        }
        if !req.references.is_empty() {
            out.push_str("Referências:\n");
            for r in &req.references {
                out.push_str("- ");
                out.push_str(r.trim());
                out.push('\n');
            }
        }
        out.push_str("[Fim do contexto]\n\n");
    }
    out.push_str(req.prompt.trim());
    if req.prompt.starts_with(HANDOFF_WRITE_PREFIX) {
        return out; // handoff step answers "ok" only: no checkpoint report requested
    }
    out.push_str(
        "\n\nAo terminar, encerre com um relatório de checkpoint: uma linha de título e no máximo 5 linhas \
         de resumo (o que mudou, estado dos testes, pendências), sem cabeçalhos markdown. Não encadeie \
         outra etapa.",
    );
    out
}

/// What to spawn for one step. The prompt travels on **stdin**, never in argv: `claude -p`
/// without a positional prompt reads it from stdin (documented in the headless guide:
/// `gh pr diff | claude -p --append-system-prompt ...`), so a step text such as
/// "--dangerously-skip-permissions x" can never be parsed as a flag, and neither the
/// ~32k-char command-line limit nor `.cmd` argument escaping applies.
///
/// The caller spawns `program args` in `cwd` with piped stdin and stdout, writes
/// `stdin` and closes it. Append `--allowedTools` (variadic) last.
#[derive(Debug, Clone, PartialEq)]
pub struct StepCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Final prompt text to write to the child's stdin.
    pub stdin: String,
}

/// A value that would be read as an option if it led an argument.
fn flag_safe(v: &str) -> Option<&str> {
    let v = v.trim();
    (!v.is_empty() && !v.starts_with('-')).then_some(v)
}

pub fn build_command(req: &StepRequest, claude_exe: &Path) -> StepCommand {
    let mut args: Vec<String> =
        vec!["-p".into(), "--output-format".into(), "stream-json".into(), "--verbose".into()];
    if let Some(id) = req.resume_session.as_deref().and_then(flag_safe) {
        args.push("--resume".into());
        args.push(id.into());
    }
    if let Some(m) = req.model.as_deref().and_then(flag_safe) {
        args.push("--model".into());
        args.push(m.into());
    }
    if let Some(pm) = req.permission_mode.as_deref().and_then(flag_safe) {
        args.push("--permission-mode".into());
        args.push(pm.into());
    }
    StepCommand { program: claude_exe.to_path_buf(), args, cwd: req.cwd.clone(), stdin: build_prompt(req) }
}

// ---------- outcome ----------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointReport {
    pub title: String,
    /// At most `MAX_REPORT_LINES`.
    pub lines: Vec<String>,
    /// True when the text had more lines (or longer ones) than were kept.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepOutcome {
    pub session_id: String,
    pub result_text: String,
    pub is_error: bool,
    pub num_turns: u64,
    pub duration_ms: u64,
    pub total_cost_usd: Option<f64>,
    pub usage: TokenTotals,
    pub report: CheckpointReport,
    /// Fraction of the context window filled by the LAST main-conversation assistant
    /// message (`None` when the stream had no assistant message).
    pub context_fill: Option<f64>,
}

fn clip(s: &str, max: usize) -> (String, bool) {
    if s.chars().count() <= max {
        return (s.to_string(), false);
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    (out, true)
}

fn clean_markdown(line: &str) -> String {
    let t = line.trim().trim_start_matches('#').trim();
    let t = t.strip_prefix("**").and_then(|x| x.strip_suffix("**")).unwrap_or(t);
    t.trim().to_string()
}

/// First non-empty line is the title (header marks and bold wrapper removed); up to
/// five following non-empty lines are kept. Anything beyond sets `truncated`.
pub fn extract_report(text: &str) -> CheckpointReport {
    let mut it = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(first) = it.next() else {
        return CheckpointReport::default();
    };
    let (title, mut truncated) = clip(&clean_markdown(first), MAX_LINE_CHARS);
    let mut lines = Vec::new();
    for l in it {
        if lines.len() == MAX_REPORT_LINES {
            truncated = true;
            break;
        }
        let (c, cut) = clip(&clean_markdown(l), MAX_LINE_CHARS);
        truncated |= cut;
        lines.push(c);
    }
    CheckpointReport { title, lines, truncated }
}

// ---------- live events ----------

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Init { session_id: String, model: Option<String> },
    AssistantText { text: String },
    /// `summary` is a short hint (file path, command, pattern) when the input has one.
    ToolUse { name: String, summary: Option<String> },
    ToolResult { is_error: bool },
    Result { is_error: bool, session_id: Option<String> },
}

fn tool_summary(input: &Value) -> Option<String> {
    for key in ["file_path", "path", "command", "pattern", "description", "url", "query"] {
        if let Some(s) = input.get(key).and_then(|v| v.as_str()) {
            let first_line = s.lines().next().unwrap_or("");
            return Some(clip(first_line, MAX_SUMMARY_CHARS).0);
        }
    }
    None
}

/// Events found in one stream line (empty for blanks, garbage and unknown types).
pub fn parse_stream_line(line: &str) -> Vec<StreamEvent> {
    let line = line.trim_start_matches('\u{feff}').trim();
    if line.is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let sid = v.get("session_id").and_then(|s| s.as_str()).map(str::to_string);
    let mut out = Vec::new();
    match v.get("type").and_then(|t| t.as_str()) {
        Some("system") if v.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
            if let Some(session_id) = sid {
                out.push(StreamEvent::Init {
                    session_id,
                    model: v.get("model").and_then(|m| m.as_str()).map(str::to_string),
                });
            }
        }
        Some("assistant") => {
            for b in blocks(&v) {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()) {
                            out.push(StreamEvent::AssistantText { text: t.to_string() });
                        }
                    }
                    Some("tool_use") => out.push(StreamEvent::ToolUse {
                        name: b.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string(),
                        summary: b.get("input").and_then(tool_summary),
                    }),
                    _ => {}
                }
            }
        }
        Some("user") => {
            for b in blocks(&v) {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    out.push(StreamEvent::ToolResult {
                        is_error: b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false),
                    });
                }
            }
        }
        Some("result") => out.push(StreamEvent::Result { is_error: result_is_error(&v), session_id: sid }),
        _ => {}
    }
    out
}

fn blocks(v: &Value) -> impl Iterator<Item = &Value> {
    v.get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
}

fn result_is_error(v: &Value) -> bool {
    v.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false)
        || v.get("subtype").and_then(|s| s.as_str()).map_or(false, |s| s != "success")
}

fn usage_of(u: &Value) -> TokenTotals {
    let n = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    TokenTotals {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_write: n("cache_creation_input_tokens"),
        cache_read: n("cache_read_input_tokens"),
    }
}

/// Reads the whole stream. A missing `result` event (process killed, crash) yields an
/// outcome with `is_error = true` that still carries the session id, so the step can
/// be resumed. Only I/O errors are returned as `Err`.
pub fn parse_stream(reader: impl BufRead) -> Result<StepOutcome> {
    let mut session_id = String::new();
    let mut result: Option<Value> = None;
    let mut init_model: Option<String> = None;
    let mut last_main: Option<(TokenTotals, Option<String>)> = None;
    // Fallback usage when `result` has none: last line per assistant message id wins.
    let mut per_msg: std::collections::HashMap<String, TokenTotals> = Default::default();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(s) = v.get("session_id").and_then(|s| s.as_str()).filter(|s| !s.is_empty()) {
            session_id = s.to_string();
        }
        match v.get("type").and_then(|t| t.as_str()) {
            Some("system") if v.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
                init_model = v.get("model").and_then(|m| m.as_str()).map(str::to_string);
            }
            Some("assistant") => {
                // Subagent messages (parent_tool_use_id set) have their own context.
                let is_main = v.get("parent_tool_use_id").map_or(true, |p| p.is_null());
                if let (true, Some(u)) = (is_main, v.pointer("/message/usage")) {
                    let model = v.pointer("/message/model").and_then(|m| m.as_str()).map(str::to_string);
                    last_main = Some((usage_of(u), model));
                }
                if let (Some(id), Some(u)) = (
                    v.pointer("/message/id").and_then(|i| i.as_str()),
                    v.pointer("/message/usage"),
                ) {
                    per_msg.insert(id.to_string(), usage_of(u));
                }
            }
            Some("result") => result = Some(v),
            _ => {}
        }
    }

    let fallback_usage = per_msg.values().fold(TokenTotals::default(), |a, u| TokenTotals {
        input: a.input + u.input,
        output: a.output + u.output,
        cache_write: a.cache_write + u.cache_write,
        cache_read: a.cache_read + u.cache_read,
    });

    let context_fill = last_main
        .as_ref()
        .map(|(u, m)| context_fill(u, m.as_deref().or(init_model.as_deref()).unwrap_or("")));

    let Some(r) = result else {
        return Ok(StepOutcome {
            session_id,
            result_text: "O stream terminou sem resultado final.".to_string(),
            is_error: true,
            num_turns: 0,
            duration_ms: 0,
            total_cost_usd: None,
            usage: fallback_usage,
            report: CheckpointReport::default(),
            context_fill,
        });
    };

    let is_error = result_is_error(&r);
    let mut text = r.get("result").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if text.trim().is_empty() {
        // Error results (e.g. error_max_turns) carry no `result` text.
        let errors: Vec<&str> = r
            .get("errors")
            .and_then(|e| e.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        text = if errors.is_empty() {
            r.get("subtype").and_then(|s| s.as_str()).unwrap_or("").to_string()
        } else {
            errors.join("\n")
        };
    }
    Ok(StepOutcome {
        session_id,
        report: extract_report(&text),
        result_text: text,
        is_error,
        num_turns: r.get("num_turns").and_then(|n| n.as_u64()).unwrap_or(0),
        duration_ms: r.get("duration_ms").and_then(|n| n.as_u64()).unwrap_or(0),
        total_cost_usd: r.get("total_cost_usd").and_then(|n| n.as_f64()),
        usage: r.get("usage").map(usage_of).unwrap_or(fallback_usage),
        context_fill,
    })
}

// ---------- permission blocks ----------

fn denial_reason(tool: &str, input: Option<&Value>) -> String {
    match input.and_then(tool_summary) {
        Some(s) => format!("Ferramenta fora da lista: {tool} ({s})"),
        None => format!("Ferramenta fora da lista: {tool}"),
    }
}

/// Reason text if this stream line says a tool was denied for lack of permission
/// (`None` otherwise). Recognised, per the headless docs ("denials appear as
/// `permission_denied` system messages, and the final result lists them in
/// `permission_denials`"): the system message, the `permission_denials` array of the
/// result, and, as a fallback, an error `tool_result` saying the tool "requested
/// permissions". The exact field names are not documented in detail, so reading is
/// tolerant (`tool_name` or `tool`, `tool_input` or `input`).
pub fn detect_permission_block_line(line: &str) -> Option<String> {
    let line = line.trim_start_matches('\u{feff}').trim();
    if line.is_empty() || !(line.contains("permission") || line.contains("approval")) {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let name_of = |o: &Value| {
        o.get("tool_name").or_else(|| o.get("tool")).and_then(|n| n.as_str()).map(str::to_string)
    };
    let input_of = |o: &Value| o.get("tool_input").or_else(|| o.get("input")).cloned();
    match v.get("type").and_then(|t| t.as_str()) {
        Some("system") if v.get("subtype").and_then(|s| s.as_str()) == Some("permission_denied") => {
            let tool = name_of(&v).unwrap_or_else(|| "?".into());
            Some(denial_reason(&tool, input_of(&v).as_ref()))
        }
        Some("result") => {
            let first = v.get("permission_denials")?.as_array()?.first()?.clone();
            let tool = name_of(&first).unwrap_or_else(|| "?".into());
            Some(denial_reason(&tool, input_of(&first).as_ref()))
        }
        Some("user") => {
            for b in blocks(&v) {
                if b.get("type").and_then(|t| t.as_str()) != Some("tool_result")
                    || !b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false)
                {
                    continue;
                }
                let text = b.get("content").and_then(|c| c.as_str()).unwrap_or("");
                if let Some(rest) = text.split("requested permissions to use ").nth(1) {
                    let tool: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    return Some(format!("Ferramenta fora da lista: {tool}"));
                }
            }
            None
        }
        _ => None,
    }
}

/// First permission block found in a whole stream.
pub fn detect_permission_block(reader: impl BufRead) -> Option<String> {
    reader.lines().map_while(|l| l.ok()).find_map(|l| detect_permission_block_line(&l))
}

// ---------- context handoff ----------

/// Hand off to a fresh session when the context is this full (user decision, CLAUDE.md).
pub const HANDOFF_THRESHOLD: f64 = 0.60;

/// File the agent writes at the worktree root and the next session reads.
pub const HANDOFF_FILE: &str = "PASSAGEM.md";

/// Marker that starts the prompt of the handoff-write step (see `build_prompt`).
const HANDOFF_WRITE_PREFIX: &str = "Escreva a passagem de contexto";

/// Context window in tokens. Source: `claude-api` skill model table (cached
/// 2026-10-06): Fable 5.x, Opus 5.5/5/4.8/4.7/4.6, Sonnet 5.5/5/4.6 and Haiku 5.5 have
/// 1M; Haiku 4.5 and older (Opus 4.5 and earlier, Sonnet 4.5 and earlier) have 200K. An id
/// carrying `[1m]` is 1M. Unknown ids get 200K, the conservative choice (hands off earlier).
pub fn context_window_tokens(model: &str) -> u64 {
    let m = model.to_ascii_lowercase();
    let has = |s: &str| m.contains(s);
    if has("[1m]") {
        return 1_000_000;
    }
    let one_m = if has("fable") || has("mythos") {
        true
    } else if has("opus") {
        has("opus-5") || has("opus-4-8") || has("opus-4-7") || has("opus-4-6")
    } else if has("sonnet") {
        has("sonnet-5") || has("sonnet-4-6")
    } else if has("haiku") {
        has("haiku-5")
    } else {
        false
    };
    if one_m {
        1_000_000
    } else {
        200_000
    }
}

/// Fraction of the context window occupied: (input + cache read + cache write) of the
/// last assistant message / window of `model`. Output tokens are not counted.
pub fn context_fill(last_usage: &TokenTotals, model: &str) -> f64 {
    let used = last_usage.input + last_usage.cache_read + last_usage.cache_write;
    used as f64 / context_window_tokens(model) as f64
}

/// True when the step finished with the context at or above `HANDOFF_THRESHOLD`.
pub fn needs_handoff(outcome: &StepOutcome) -> bool {
    outcome.context_fill.map_or(false, |f| f >= HANDOFF_THRESHOLD)
}

/// Extra step, resuming the full session, asking the agent to write `PASSAGEM.md`.
/// Same cwd/model/permissions as the step it follows.
pub fn handoff_write_request(prev: &StepRequest, session_id: &str) -> StepRequest {
    StepRequest {
        cwd: prev.cwd.clone(),
        prompt: format!(
            "{HANDOFF_WRITE_PREFIX}: crie ou atualize {HANDOFF_FILE} na raiz do worktree (pasta atual), \
             com no máximo 60 linhas, para uma sessão nova continuar o trabalho. Inclua: objetivo atual, \
             decisões e regras recentes, estado do código (arquivos e último commit), o que falta fazer e \
             armadilhas conhecidas. Não altere mais nada e não faça commit. Responda apenas \"ok\"."
        ),
        context_instructions: None,
        references: Vec::new(),
        resume_session: Some(session_id.to_string()),
        model: prev.model.clone(),
        permission_mode: prev.permission_mode.clone(),
    }
}

/// Next step in a NEW session (no `--resume`): headless has no `/clear`, so starting
/// without `--resume` is the clear. `prev` is the request of the step to run next;
/// its context instructions and references are kept, the prompt gets the read-first line.
pub fn handoff_resume_request(prev: &StepRequest) -> StepRequest {
    let mut next = prev.clone();
    next.resume_session = None;
    next.prompt = format!("Leia {HANDOFF_FILE} e continue a partir dele.\n\n{}", prev.prompt.trim());
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const OK: &str = include_str!("../tests/fixtures/stream-success.jsonl");
    const ERR: &str = include_str!("../tests/fixtures/stream-error.jsonl");
    const LONG: &str = include_str!("../tests/fixtures/stream-long.jsonl");

    fn req() -> StepRequest {
        StepRequest {
            cwd: PathBuf::from("C:/proj/demo"),
            prompt: "  Corrija o teste que falha.  ".into(),
            ..Default::default()
        }
    }

    #[test]
    fn command_minimal() {
        let c = build_command(&req(), Path::new("C:/bin/claude.exe"));
        assert_eq!(c.program, PathBuf::from("C:/bin/claude.exe"));
        assert_eq!(c.cwd, PathBuf::from("C:/proj/demo"));
        assert_eq!(c.args, ["-p", "--output-format", "stream-json", "--verbose"]);
        // the prompt is on stdin, not in argv
        assert!(c.stdin.starts_with("Corrija o teste que falha."));
        assert!(c.stdin.contains("no máximo 5 linhas"));
        assert!(!c.stdin.contains("[Contexto da Nyx]"));
        assert!(!c.args.iter().any(|a| a.contains("Corrija")));
    }

    #[test]
    fn command_with_everything() {
        let mut r = req();
        r.context_instructions = Some("Responda em português.".into());
        r.references = vec!["docs/PLANO.md".into(), "CLAUDE.md".into()];
        r.resume_session = Some("sess-9".into());
        r.model = Some("opus".into());
        r.permission_mode = Some("acceptEdits".into());
        let c = build_command(&r, Path::new("claude"));
        let args = &c.args;
        let pos = |f: &str| args.iter().position(|a| a == f).unwrap();
        assert_eq!(args[pos("--resume") + 1], "sess-9");
        assert_eq!(args[pos("--model") + 1], "opus");
        assert_eq!(args[pos("--permission-mode") + 1], "acceptEdits");
        let p = &c.stdin;
        assert!(p.starts_with("[Contexto da Nyx]\nInstruções permanentes:\nResponda em português."));
        assert!(p.contains("- docs/PLANO.md\n- CLAUDE.md\n[Fim do contexto]"));
        assert!(p.find("Corrija").unwrap() > p.find("[Fim do contexto]").unwrap());
        assert!(p.trim_end().ends_with("outra etapa."));
    }

    #[test]
    fn dash_prompts_never_become_flags() {
        for evil in [
            "--dangerously-skip-permissions x",
            "-p",
            "--allowedTools Bash",
            "--mcp-config evil.json",
        ] {
            let mut r = req();
            r.prompt = evil.into();
            let c = build_command(&r, Path::new("claude"));
            assert_eq!(c.args, ["-p", "--output-format", "stream-json", "--verbose"], "{evil}");
            assert!(c.stdin.starts_with(evil));
            // feedback / handoff paths too
            let h = handoff_resume_request(&r);
            let c = build_command(&h, Path::new("claude"));
            assert!(!c.args.iter().any(|a| a.contains("dangerously") || a.contains("evil")));
        }
        // dash-leading option values are dropped instead of being read as flags
        let mut r = req();
        r.model = Some("--dangerously-skip-permissions".into());
        r.resume_session = Some("-x".into());
        r.permission_mode = Some("--add-dir".into());
        let c = build_command(&r, Path::new("claude"));
        assert_eq!(c.args, ["-p", "--output-format", "stream-json", "--verbose"]);
    }

    #[test]
    fn empty_optionals_are_skipped() {
        let mut r = req();
        r.resume_session = Some(String::new());
        r.context_instructions = Some("  ".into());
        let c = build_command(&r, Path::new("claude"));
        assert!(!c.args.iter().any(|a| a == "--resume"));
        assert!(!c.stdin.contains("[Contexto"));
    }

    #[test]
    fn report_extraction() {
        let r = extract_report("\n\n## **Etapa 2 concluída**\n\n- 3 arquivos alterados\nTestes passando\n");
        assert_eq!(r.title, "Etapa 2 concluída");
        assert_eq!(r.lines, ["- 3 arquivos alterados", "Testes passando"]);
        assert!(!r.truncated);
        assert_eq!(extract_report("   \n "), CheckpointReport::default());
        let seven = (1..=8).map(|i| format!("l{i}")).collect::<Vec<_>>().join("\n");
        let r = extract_report(&seven);
        assert_eq!((r.title.as_str(), r.lines.len(), r.truncated), ("l1", 5, true));
        let exact = (1..=6).map(|i| format!("l{i}")).collect::<Vec<_>>().join("\n");
        assert!(!extract_report(&exact).truncated);
    }

    #[test]
    fn success_stream() {
        let o = parse_stream(Cursor::new(OK)).unwrap();
        assert_eq!(o.session_id, "sess-ok-1");
        assert!(!o.is_error);
        assert_eq!((o.num_turns, o.duration_ms), (3, 8421));
        assert_eq!(o.total_cost_usd, Some(0.0421));
        assert_eq!(
            o.usage,
            TokenTotals { input: 12, output: 640, cache_write: 4100, cache_read: 21000 }
        );
        assert_eq!(o.report.title, "Etapa 1 concluída");
        assert_eq!(o.report.lines.len(), 3);
        assert!(!o.report.truncated);
    }

    #[test]
    fn error_stream() {
        let o = parse_stream(Cursor::new(ERR)).unwrap();
        assert!(o.is_error);
        assert_eq!(o.session_id, "sess-err-1");
        assert_eq!(o.result_text, "error_max_turns");
        assert_eq!(o.report.title, "error_max_turns");
        assert_eq!(o.total_cost_usd, Some(0.5));
    }

    #[test]
    fn long_result_is_truncated() {
        let o = parse_stream(Cursor::new(LONG)).unwrap();
        assert!(!o.is_error);
        assert_eq!(o.report.lines.len(), MAX_REPORT_LINES);
        assert!(o.report.truncated);
        assert!(o.result_text.lines().count() > 7);
    }

    #[test]
    fn missing_result_keeps_session_for_resume() {
        let cut: String = OK.lines().take(3).collect::<Vec<_>>().join("\n");
        let o = parse_stream(Cursor::new(cut)).unwrap();
        assert!(o.is_error);
        assert_eq!(o.session_id, "sess-ok-1");
        assert!(o.usage.output > 0, "falls back to summed assistant usage");
        assert_eq!(parse_stream(Cursor::new("")).unwrap().session_id, "");
    }

    #[test]
    fn tolerates_garbage() {
        let s = format!("\u{feff}not json\n{{\"type\":\"mystery\"}}\n\n{OK}\n[1,2]\n");
        assert_eq!(parse_stream(Cursor::new(s)).unwrap().session_id, "sess-ok-1");
    }

    const BLOCKED: &str = include_str!("../tests/fixtures/stream-blocked.jsonl");

    #[test]
    fn permission_block_detection() {
        let lines: Vec<&str> = BLOCKED.lines().collect();
        // each of the three signals works on its own
        assert_eq!(
            detect_permission_block_line(lines[2]).as_deref(),
            Some("Ferramenta fora da lista: Bash")
        );
        let want = "Ferramenta fora da lista: Bash (npm run deploy)";
        assert_eq!(detect_permission_block_line(lines[3]).as_deref(), Some(want));
        assert_eq!(detect_permission_block_line(lines[4]).as_deref(), Some(want));
        assert_eq!(detect_permission_block(Cursor::new(BLOCKED)).as_deref(), Some("Ferramenta fora da lista: Bash"));
        // normal streams, garbage and blanks do not trigger
        assert!(detect_permission_block(Cursor::new(OK)).is_none());
        assert!(detect_permission_block(Cursor::new(ERR)).is_none());
        assert!(detect_permission_block_line("permission garbage").is_none());
        assert!(detect_permission_block_line("").is_none());
        // a clean result with empty denials is not a block
        assert!(detect_permission_block_line(r#"{"type":"result","permission_denials":[]}"#).is_none());
        // the stream itself still parses as a finished (non-error) step
        assert!(!parse_stream(Cursor::new(BLOCKED)).unwrap().is_error);
    }

    const FILL: &str =include_str!("../tests/fixtures/stream-fill.jsonl");

    #[test]
    fn windows_by_model() {
        assert_eq!(context_window_tokens("claude-opus-5-5"), 1_000_000);
        assert_eq!(context_window_tokens("claude-fable-5-1"), 1_000_000);
        assert_eq!(context_window_tokens("claude-sonnet-4-6"), 1_000_000);
        assert_eq!(context_window_tokens("claude-haiku-4-5-20251001"), 200_000);
        assert_eq!(context_window_tokens("claude-opus-4-5"), 200_000);
        assert_eq!(context_window_tokens("claude-sonnet-4-5"), 200_000);
        assert_eq!(context_window_tokens("claude-sonnet-4-5[1m]"), 1_000_000);
        assert_eq!(context_window_tokens("???"), 200_000);
    }

    #[test]
    fn fill_math() {
        let u = TokenTotals { input: 100, output: 9999, cache_write: 50_000, cache_read: 99_900 };
        assert!((context_fill(&u, "claude-haiku-4-5") - 0.75).abs() < 1e-9);
        assert!((context_fill(&u, "claude-opus-5-5") - 0.15).abs() < 1e-9);
    }

    #[test]
    fn fill_from_stream_uses_last_main_message() {
        let o = parse_stream(Cursor::new(FILL)).unwrap();
        // last main message: 2 + 10000 + 600000 = 610002 of 1M; the trailing subagent
        // message (parent_tool_use_id set) is ignored.
        let f = o.context_fill.unwrap();
        assert!((f - 0.610002).abs() < 1e-9, "{f}");
        assert!(needs_handoff(&o));
        let ok = parse_stream(Cursor::new(OK)).unwrap();
        assert!(ok.context_fill.unwrap() < 0.02 && !needs_handoff(&ok));
        assert!(parse_stream(Cursor::new("")).unwrap().context_fill.is_none());
    }

    #[test]
    fn threshold_boundary() {
        let mut o = parse_stream(Cursor::new(OK)).unwrap();
        o.context_fill = Some(HANDOFF_THRESHOLD);
        assert!(needs_handoff(&o));
        o.context_fill = Some(0.599);
        assert!(!needs_handoff(&o));
    }

    #[test]
    fn handoff_write_resumes_and_asks_only_ok() {
        let mut prev = req();
        prev.model = Some("opus".into());
        prev.permission_mode = Some("acceptEdits".into());
        prev.context_instructions = Some("instr".into());
        let w = handoff_write_request(&prev, "sess-fill-1");
        assert_eq!(w.resume_session.as_deref(), Some("sess-fill-1"));
        assert_eq!(w.model.as_deref(), Some("opus"));
        assert_eq!(w.permission_mode.as_deref(), Some("acceptEdits"));
        let c = build_command(&w, Path::new("claude"));
        let (args, cwd) = (&c.args, &c.cwd);
        assert_eq!(cwd, &prev.cwd);
        assert_eq!(args[args.iter().position(|a| a == "--resume").unwrap() + 1], "sess-fill-1");
        assert!(c.stdin.contains("PASSAGEM.md") && c.stdin.contains("60 linhas"));
        assert!(c.stdin.contains("Responda apenas \"ok\""));
        assert!(!c.stdin.contains("relatório de checkpoint"), "no checkpoint suffix");
        assert!(!c.stdin.contains("[Contexto da Nyx]"));
    }

    #[test]
    fn handoff_resume_is_a_fresh_session() {
        let mut next = req();
        next.resume_session = Some("old".into());
        next.context_instructions = Some("instr".into());
        next.references = vec!["CLAUDE.md".into()];
        let r = handoff_resume_request(&next);
        assert!(r.resume_session.is_none());
        let c = build_command(&r, Path::new("claude"));
        assert!(!c.args.iter().any(|a| a == "--resume"));
        let p = &c.stdin;
        assert!(p.contains("Leia PASSAGEM.md e continue a partir dele.\n\nCorrija o teste"));
        assert!(p.contains("instr") && p.contains("- CLAUDE.md"));
        assert!(p.contains("relatório de checkpoint"));
    }

    #[test]
    fn live_events() {
        let ev: Vec<StreamEvent> = OK.lines().flat_map(parse_stream_line).collect();
        assert!(matches!(&ev[0], StreamEvent::Init { session_id, model } if session_id == "sess-ok-1" && model.as_deref() == Some("claude-opus-5-5")));
        assert!(ev.contains(&StreamEvent::ToolUse { name: "Bash".into(), summary: Some("cargo test".into()) }));
        assert!(ev.contains(&StreamEvent::ToolUse { name: "Edit".into(), summary: Some("src/lib.rs".into()) }));
        assert!(ev.contains(&StreamEvent::ToolResult { is_error: false }));
        assert!(ev.iter().any(|e| matches!(e, StreamEvent::AssistantText { .. })));
        assert!(matches!(ev.last().unwrap(), StreamEvent::Result { is_error: false, .. }));
        assert!(parse_stream_line("garbage").is_empty());
        assert!(parse_stream_line("{\"type\":\"system\",\"subtype\":\"hook_started\"}").is_empty());
    }
}
