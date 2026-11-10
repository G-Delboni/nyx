//! Nyx AI (opt-in). Spec: docs/specs/NOITE-2.md §4. Owner: agent B.
//!
//! Blocking client for the Anthropic Messages API plus the three things Nyx asks the
//! model: `chat` (with `propose_*` tools that become `ProposedAction`s, never executed
//! here), `summarize_step` and `plan_steps`. The API key comes from `ANTHROPIC_API_KEY`;
//! the base URL can be overridden with `NYX_AI_BASE_URL` (tests point it at a fake server).
//! Only compact, caller-provided text is sent: no transcripts, no code.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

mod prompts;
mod tools;

pub use tools::{actions_from_blocks, tool_definitions, ProposedAction};

/// Haiku model id (current Haiku in the `claude-api` skill, 2026-11-07).
pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
pub const API_VERSION: &str = "2023-06-01";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const MAX_HISTORY: usize = 20;
pub const MAX_ATTEMPTS: u32 = 3;
pub const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("NYX_ANTHROPIC_API_KEY não definida")]
    NoKey,
    #[error("falha de rede: {0}")]
    Transport(String),
    #[error("API respondeu {status} ({kind}): {message}")]
    Api { status: u16, kind: String, message: String },
    #[error("resposta inesperada da API: {0}")]
    BadResponse(String),
}

pub type Result<T> = std::result::Result<T, AiError>;

// ---------- usage and cost ----------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

/// USD per million tokens: (input, output, cache write 5m, cache read). Source: the
/// `claude-api` skill model table (2026-11-07). Unknown models are priced as Sonnet 5.5
/// (conservative, never cheaper than Haiku).
fn price(model: &str) -> (f64, f64, f64, f64) {
    let m = model.to_ascii_lowercase();
    if m.contains("haiku-5") {
        (0.10, 0.50, 0.125, 0.01)
    } else if m.contains("haiku-4") {
        (1.0, 5.0, 1.25, 0.10)
    } else if m.contains("opus-5-5") {
        (4.0, 20.0, 5.0, 0.20)
    } else if m.contains("opus") {
        (5.0, 25.0, 6.25, 0.50)
    } else {
        (2.0, 10.0, 2.5, 0.20)
    }
}

pub fn cost_usd(usage: &Usage, model: &str) -> f64 {
    let (i, o, w, r) = price(model);
    (usage.input_tokens as f64 * i
        + usage.output_tokens as f64 * o
        + usage.cache_creation_input_tokens as f64 * w
        + usage.cache_read_input_tokens as f64 * r)
        / 1_000_000.0
}

// ---------- key ----------

/// Nyx's own variable comes first: a global `ANTHROPIC_API_KEY` would also make every
/// Claude Code session bill the API instead of the user's plan, so the user is told to
/// set `NYX_ANTHROPIC_API_KEY`. `ANTHROPIC_API_KEY` stays as a fallback.
pub const KEY_ENV: &str = "NYX_ANTHROPIC_API_KEY";

fn env_key() -> Option<String> {
    [KEY_ENV, "ANTHROPIC_API_KEY"]
        .iter()
        .find_map(|v| std::env::var(v).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()))
}

/// True when `NYX_ANTHROPIC_API_KEY` (or `ANTHROPIC_API_KEY`) is set and non-empty.
pub fn key_present() -> bool {
    env_key().is_some()
}

// ---------- wire types ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One turn of chat history (plain text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    /// thinking blocks and anything else we do not use
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MessagesResponse {
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Usage,
    #[serde(default)]
    pub model: Option<String>,
}

impl MessagesResponse {
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }
}

// ---------- client ----------

#[derive(Clone)]
pub struct Client {
    api_key: String,
    pub base_url: String,
    pub model: String,
    pub timeout: Duration,
    pub max_attempts: u32,
    /// First retry delay; doubles each attempt (a `retry-after` header wins, capped at 5 s).
    pub backoff: Duration,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("api_key", &"<redacted>")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

impl Client {
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Client {
            api_key: api_key.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: DEFAULT_MODEL.to_string(),
            timeout: TIMEOUT,
            max_attempts: MAX_ATTEMPTS,
            backoff: Duration::from_millis(800),
        }
    }

    /// Key from `ANTHROPIC_API_KEY`, base URL from `NYX_AI_BASE_URL` (default: the real API).
    pub fn from_env() -> Result<Self> {
        let key = env_key().ok_or(AiError::NoKey)?;
        let base = std::env::var("NYX_AI_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Ok(Client::new(key, base))
    }

    /// POST /v1/messages with retries on 429, 529, 5xx and network errors.
    pub fn messages(&self, body: &Value) -> Result<MessagesResponse> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let url = format!("{}/v1/messages", self.base_url);
        let attempts = self.max_attempts.max(1);
        let mut last: Option<AiError> = None;
        let mut retry_after: Option<u64> = None;
        for attempt in 0..attempts {
            if attempt > 0 {
                let wait = retry_after
                    .take()
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| self.backoff * 2u32.pow(attempt - 1));
                std::thread::sleep(wait.min(Duration::from_secs(5)));
            }
            let sent = agent
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION)
                .send_json(body);
            let mut resp = match sent {
                Ok(r) => r,
                Err(e) => {
                    last = Some(AiError::Transport(e.to_string()));
                    continue;
                }
            };
            let status = resp.status().as_u16();
            let header_retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok());
            let text = resp.body_mut().read_to_string().unwrap_or_default();
            if (200..300).contains(&status) {
                return serde_json::from_str(&text).map_err(|e| AiError::BadResponse(e.to_string()));
            }
            let (kind, message) = serde_json::from_str::<Value>(&text)
                .ok()
                .map(|v| {
                    (
                        v.pointer("/error/type").and_then(|x| x.as_str()).unwrap_or("unknown").to_string(),
                        v.pointer("/error/message").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    )
                })
                .unwrap_or_else(|| ("unknown".to_string(), text.chars().take(200).collect()));
            let err = AiError::Api { status, kind, message };
            let retryable = status == 429 || status == 529 || status >= 500;
            if !retryable {
                return Err(err);
            }
            retry_after = header_retry_after;
            last = Some(err);
        }
        Err(last.unwrap_or_else(|| AiError::Transport("sem tentativas".into())))
    }
}

// ---------- high-level calls ----------

#[derive(Debug, Clone, PartialEq)]
pub struct ChatReply {
    pub text: String,
    pub actions: Vec<ProposedAction>,
    pub usage: Usage,
    pub cost_usd: f64,
}

/// Text produced by `summarize_step`, with what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Generated {
    pub text: String,
    pub usage: Usage,
    pub cost_usd: f64,
}

/// Steps produced by `plan_steps`, with what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub steps: Vec<String>,
    pub usage: Usage,
    pub cost_usd: f64,
}

fn base_request(model: &str, system: &str, max_tokens: u32, messages: Vec<Value>) -> Value {
    // No sampling params (rejected on current models); low effort keeps Haiku cheap.
    json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "output_config": {"effort": "low"},
        "messages": messages,
    })
}

/// History -> API messages: last `MAX_HISTORY` turns, same-role neighbours merged, must
/// start with `user` and end with the new user text.
pub fn build_messages(history: &[ChatTurn], user_text: &str) -> Vec<Value> {
    let start = history.len().saturating_sub(MAX_HISTORY);
    let mut turns: Vec<(Role, String)> = Vec::new();
    for t in &history[start..] {
        if t.text.trim().is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some((r, s)) if *r == t.role => {
                s.push('\n');
                s.push_str(t.text.trim());
            }
            _ => turns.push((t.role, t.text.trim().to_string())),
        }
    }
    while turns.first().map_or(false, |(r, _)| *r == Role::Assistant) {
        turns.remove(0);
    }
    match turns.last_mut() {
        Some((Role::User, s)) => {
            s.push('\n');
            s.push_str(user_text.trim());
        }
        _ => turns.push((Role::User, user_text.trim().to_string())),
    }
    turns
        .into_iter()
        .map(|(r, s)| json!({"role": if r == Role::User { "user" } else { "assistant" }, "content": s}))
        .collect()
}

impl Client {
    /// Chat with Nyx. `snapshot` is the compact local state written by the caller.
    /// Proposed actions are only proposals: nothing is executed here.
    pub fn chat(&self, snapshot: &str, history: &[ChatTurn], user_text: &str) -> Result<ChatReply> {
        let system = prompts::chat_system(snapshot);
        let mut body = base_request(&self.model, &system, 700, build_messages(history, user_text));
        body["tools"] = tool_definitions();
        body["tool_choice"] = json!({"type": "auto"});
        let resp = self.messages(&body)?;
        let actions = actions_from_blocks(&resp.content);
        let mut text = resp.text();
        if text.is_empty() && !actions.is_empty() {
            text = "Preparei uma ação. Confirme abaixo se quiser seguir.".to_string();
        }
        Ok(ChatReply { text, actions, cost_usd: cost_usd(&resp.usage, &self.model), usage: resp.usage })
    }

    /// Short review of a finished step: at most 5 lines.
    pub fn summarize_step(&self, report: &str, diff_stat: &str) -> Result<Generated> {
        let user = prompts::summary_user(report, diff_stat);
        let body = base_request(&self.model, prompts::SUMMARY_SYSTEM, 400, vec![json!({"role":"user","content":user})]);
        let resp = self.messages(&body)?;
        let text = clamp_lines(&resp.text(), 5);
        if text.is_empty() {
            return Err(AiError::BadResponse("resumo vazio".into()));
        }
        Ok(Generated { text, cost_usd: cost_usd(&resp.usage, &self.model), usage: resp.usage })
    }

    /// 3 to 8 short steps for a goal, one per line in the model's answer.
    pub fn plan_steps(&self, goal: &str, context: &str) -> Result<Planned> {
        let user = prompts::plan_user(goal, context);
        let body = base_request(&self.model, prompts::PLAN_SYSTEM, 600, vec![json!({"role":"user","content":user})]);
        let resp = self.messages(&body)?;
        let steps = parse_steps(&resp.text());
        if steps.is_empty() {
            return Err(AiError::BadResponse("plano vazio".into()));
        }
        Ok(Planned { steps, cost_usd: cost_usd(&resp.usage, &self.model), usage: resp.usage })
    }
}

/// Non-empty lines, trimmed, at most `n`.
pub fn clamp_lines(text: &str, n: usize) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).take(n).collect::<Vec<_>>().join("\n")
}

/// One step per line; strips numbering and bullets; keeps at most 8.
pub fn parse_steps(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            let t = l.trim().trim_start_matches(['-', '*', '•']).trim();
            let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
            let t = if digits > 0 && t[digits..].starts_with(['.', ')']) { t[digits + 1..].trim() } else { t };
            t.trim_matches('*').trim().to_string()
        })
        .filter(|l| !l.is_empty())
        .take(8)
        .collect()
}
