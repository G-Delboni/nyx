//! Tipos do contrato UI <-> núcleo. Espelho de `app/src/api/contract.ts`
//! (fonte da verdade dos nomes). Não renomear campos sem mudar o contrato.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Waiting,
    Done,
    Ended,
    Idle,
    Error,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Done => "done",
            Self::Ended => "ended",
            Self::Idle => "idle",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "done" => Self::Done,
            "ended" => Self::Ended,
            "error" => Self::Error,
            _ => Self::Idle,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

impl TokenCounts {
    pub fn add(&mut self, o: &TokenCounts) {
        self.input += o.input;
        self.output += o.output;
        self.cache_write += o.cache_write;
        self.cache_read += o.cache_read;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub session_id: String,
    pub agent: String, // "claude-code"
    pub cwd: Option<String>,
    pub folder_name: Option<String>,
    pub context_id: Option<String>,
    pub status: SessionStatus,
    pub started_at: String,
    pub updated_at: String,
    pub last_message: Option<String>,
    pub tokens: TokenCounts,
    pub cost_usd: f64,
}

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
    UsageThreshold,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TaskDone => "taskDone",
            Self::NeedsInput => "needsInput",
            Self::Error => "error",
            Self::SessionStart => "sessionStart",
            Self::SessionEnd => "sessionEnd",
            Self::PromptSubmitted => "promptSubmitted",
            Self::ToolUse => "toolUse",
            Self::UsageThreshold => "usageThreshold",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "taskDone" => Self::TaskDone,
            "needsInput" => Self::NeedsInput,
            "error" => Self::Error,
            "sessionStart" => Self::SessionStart,
            "sessionEnd" => Self::SessionEnd,
            "promptSubmitted" => Self::PromptSubmitted,
            "toolUse" => Self::ToolUse,
            "usageThreshold" => Self::UsageThreshold,
            _ => return None,
        })
    }

    pub fn from_claude(k: nyx_claude::EventKind) -> Self {
        use nyx_claude::EventKind as C;
        match k {
            C::TaskDone => Self::TaskDone,
            C::NeedsInput => Self::NeedsInput,
            C::Error => Self::Error,
            C::SessionStart => Self::SessionStart,
            C::SessionEnd => Self::SessionEnd,
            C::PromptSubmitted => Self::PromptSubmitted,
            C::ToolUse => Self::ToolUse,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "high" => Self::High,
            "medium" => Self::Medium,
            _ => Self::Low,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NyxEvent {
    pub id: i64,
    pub kind: EventKind,
    pub priority: Priority,
    pub session_id: Option<String>,
    pub context_id: Option<String>,
    pub title: String,
    pub detail: Option<String>,
    pub created_at: String,
    pub read: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageRange {
    Session,
    Today,
    Week,
}

impl UsageRange {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Today => "today",
            Self::Week => "week",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    pub tokens: TokenCounts,
    pub cost_usd: f64,
    pub messages: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub range: UsageRange,
    pub context_id: Option<String>,
    pub from: String,
    pub to: String,
    pub tokens: TokenCounts,
    pub cost_usd: f64,
    pub sessions: u64,
    pub messages: u64,
    pub by_model: Vec<ModelUsage>,
    pub ceiling_usd: Option<f64>,
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContext {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub root_path: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub created_at: String,
    // Night 2
    /// Entradas de `claude --allowedTools` (ex.: "Read", "Edit", "Bash(npm test:*)").
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Caminhos/notas injetados em todo prompt de delegação.
    #[serde(default)]
    pub references: Vec<String>,
    /// Resumos/planos com IA por padrão neste contexto (opt-in).
    #[serde(default)]
    pub ai_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanPreset {
    Pro,
    #[serde(rename = "max5x")]
    Max5x,
    #[serde(rename = "max20x")]
    Max20x,
    Custom,
}

impl PlanPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pro => "pro",
            Self::Max5x => "max5x",
            Self::Max20x => "max20x",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DndSchedule {
    pub start: String, // "22:00"
    pub end: String,   // "08:00"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub plan_preset: PlanPreset,
    pub weekly_ceiling_usd: f64,
    pub alert_thresholds: Vec<f64>,
    pub do_not_disturb: bool,
    pub dnd_schedule: Option<DndSchedule>,
    pub autostart: bool,
    #[serde(default)]
    pub data_dir: String,
    /// `Stop` depois de tanto tempo seguido em `running` vira toast de tarefa longa (média).
    #[serde(default = "default_long_task_minutes")]
    pub long_task_minutes: u32,
    /// Cena do topo do painel: retrato (padrão, direção E) ou avatar 3D (beta).
    /// JSON antigo sem o campo, ou com valor desconhecido, vira `portrait`.
    #[serde(default, deserialize_with = "lenient_scene_mode")]
    pub scene_mode: SceneMode,
    /// Pass ASCII do avatar: sempre (`on`), só na intro (`introOnly`) ou nunca (`off`).
    /// JSON antigo sem o campo, ou com valor desconhecido, vira `on`.
    #[serde(default, deserialize_with = "lenient_ascii_mode")]
    pub ascii_mode: AsciiMode,
    // Voz (TTS). JSON antigo sem os campos recebe os padrões do contrato.
    #[serde(default = "default_true")]
    pub voice_enabled: bool,
    #[serde(default = "default_engine", deserialize_with = "lenient_engine")]
    pub voice_engine: nyx_voice::EnginePref,
    #[serde(default = "default_voice_name")]
    pub voice_name: String,
    #[serde(default = "default_voice_volume")]
    pub voice_volume: f64,
    #[serde(default = "default_voice_prosody")]
    pub voice_rate: i32,
    #[serde(default = "default_voice_prosody")]
    pub voice_pitch: i32,
    #[serde(default)]
    pub voice_speak_on: VoiceSpeakOn,
    #[serde(default)]
    pub voice_muted: bool,
}

/// Onde a Nyx fala (cada um liga/desliga). Campo ausente = ligado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSpeakOn {
    #[serde(default = "default_true")]
    pub alerts: bool,
    #[serde(default = "default_true")]
    pub opening: bool,
    #[serde(default = "default_true")]
    pub chat: bool,
    #[serde(default = "default_true")]
    pub long_task: bool,
}

impl Default for VoiceSpeakOn {
    fn default() -> Self {
        Self { alerts: true, opening: true, chat: true, long_task: true }
    }
}

fn default_true() -> bool {
    true
}

fn default_engine() -> nyx_voice::EnginePref {
    nyx_voice::EnginePref::Auto
}

fn lenient_engine<'de, D: serde::Deserializer<'de>>(d: D) -> Result<nyx_voice::EnginePref, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or(nyx_voice::EnginePref::Auto))
}

fn default_voice_name() -> String {
    nyx_voice::DEFAULT_VOICE.to_string()
}

pub const DEFAULT_VOICE_VOLUME: f64 = 0.8;

fn default_voice_volume() -> f64 {
    DEFAULT_VOICE_VOLUME
}

pub const DEFAULT_VOICE_PROSODY: i32 = -5;

fn default_voice_prosody() -> i32 {
    DEFAULT_VOICE_PROSODY
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpeechKind {
    Alert,
    Opening,
    Chat,
    LongTask,
    Test,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechEvent {
    pub id: String,
    pub kind: SpeechKind,
    pub text: String,
    pub engine: nyx_voice::Engine,
    pub duration_ms: u64,
    pub envelope: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    /// "azure" | "windows" | "none"
    pub engine_in_use: String,
    pub azure_key_present: bool,
    pub azure_region: Option<String>,
    pub windows_voice: Option<String>,
    pub month_chars: u64,
    pub free_tier_chars: u64,
    pub speaking: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsciiMode {
    #[default]
    #[serde(rename = "on")]
    On,
    #[serde(rename = "introOnly")]
    IntroOnly,
    #[serde(rename = "off")]
    Off,
}

fn lenient_ascii_mode<'de, D: serde::Deserializer<'de>>(d: D) -> Result<AsciiMode, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SceneMode {
    #[default]
    #[serde(rename = "portrait")]
    Portrait,
    #[serde(rename = "avatar3d")]
    Avatar3d,
}

fn lenient_scene_mode<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SceneMode, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

pub const DEFAULT_LONG_TASK_MINUTES: u32 = 10;

fn default_long_task_minutes() -> u32 {
    DEFAULT_LONG_TASK_MINUTES
}

impl Settings {
    /// Padrões da Fase 0: preset max5x, alertas em 80/95, autostart desligado.
    pub fn defaults(weekly_ceiling_usd: f64) -> Self {
        Self {
            plan_preset: PlanPreset::Max5x,
            weekly_ceiling_usd,
            alert_thresholds: vec![80.0, 95.0],
            do_not_disturb: false,
            dnd_schedule: None,
            autostart: false,
            data_dir: String::new(),
            long_task_minutes: DEFAULT_LONG_TASK_MINUTES,
            scene_mode: SceneMode::Portrait,
            ascii_mode: AsciiMode::On,
            voice_enabled: true,
            voice_engine: nyx_voice::EnginePref::Auto,
            voice_name: nyx_voice::DEFAULT_VOICE.to_string(),
            voice_volume: DEFAULT_VOICE_VOLUME,
            voice_rate: DEFAULT_VOICE_PROSODY,
            voice_pitch: DEFAULT_VOICE_PROSODY,
            voice_speak_on: VoiceSpeakOn::default(),
            voice_muted: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NyxStatus {
    Active,
    Analyzing,
    Alert,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub status: NyxStatus,
    pub active_delegations: u64,
    pub agents: Vec<AgentSession>,
    pub unread_events: u64,
    pub week_usage: UsageSummary,
    pub settings: Settings,
    pub contexts: Vec<ProjectContext>,
}

// ---------- Night 2: delegação ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DelegationStatus {
    Draft,
    Running,
    Checkpoint,
    Handoff,
    Blocked,
    Failed,
    Done,
    Cancelled,
}

impl DelegationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::Checkpoint => "checkpoint",
            Self::Handoff => "handoff",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "checkpoint" => Self::Checkpoint,
            "handoff" => Self::Handoff,
            "blocked" => Self::Blocked,
            "failed" => Self::Failed,
            "done" => Self::Done,
            "cancelled" => Self::Cancelled,
            _ => Self::Draft,
        }
    }

    /// Conta para o limite de 1 ativa por contexto.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Checkpoint | Self::Handoff | Self::Blocked | Self::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationDraft {
    pub context_id: String,
    pub title: String,
    pub steps: Vec<String>,
    #[serde(default)]
    pub use_ai: bool,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Running,
    Checkpoint,
    Approved,
    Failed,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Checkpoint => "checkpoint",
            Self::Approved => "approved",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "checkpoint" => Self::Checkpoint,
            "approved" => Self::Approved,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepReport {
    pub title: String,
    pub lines: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationStep {
    pub index: u32,
    pub prompt: String,
    pub status: StepStatus,
    pub attempts: u32,
    pub report: Option<StepReport>,
    pub ai_summary: Option<String>,
    pub feedback: Vec<String>,
    pub commit: Option<String>,
    pub cost_usd: f64,
    pub context_fill: Option<f64>,
    pub live_activity: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delegation {
    pub id: String,
    pub context_id: String,
    pub title: String,
    pub status: DelegationStatus,
    pub current_step: u32,
    pub steps: Vec<DelegationStep>,
    pub worktree_path: String,
    pub branch: String,
    pub base_branch: String,
    pub claude_session_id: Option<String>,
    pub handoffs: u32,
    pub blocked_reason: Option<String>,
    pub cost_usd: f64,
    pub created_at: String,
    pub updated_at: String,
}

// ---------- Night 2: chat ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatActionKind {
    StartDelegation,
    ApproveStep,
    AdjustStep,
    InstallHooks,
    OpenPanel,
    CreateContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatActionState {
    Proposed,
    Done,
    Failed,
    Dismissed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAction {
    pub id: String,
    pub kind: ChatActionKind,
    pub label: String,
    pub args: serde_json::Map<String, serde_json::Value>,
    pub state: ChatActionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Nyx,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub context_id: Option<String>,
    pub role: ChatRole,
    pub text: String,
    pub actions: Vec<ChatAction>,
    pub cost_usd: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub key_present: bool,
    pub model: String,
    pub week_cost_usd: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_contract_names() {
        let s = Settings::defaults(100.0);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["planPreset"], "max5x");
        assert_eq!(v["weeklyCeilingUsd"], 100.0);
        assert_eq!(v["doNotDisturb"], false);
        assert!(v["dndSchedule"].is_null());
        let t = serde_json::to_value(TokenCounts { cache_write: 3, ..Default::default() }).unwrap();
        assert_eq!(t["cacheWrite"], 3);
        assert_eq!(serde_json::to_value(EventKind::UsageThreshold).unwrap(), "usageThreshold");
        assert_eq!(serde_json::to_value(SessionStatus::Waiting).unwrap(), "waiting");
        assert_eq!(serde_json::to_value(UsageRange::Week).unwrap(), "week");
        let back: Settings = serde_json::from_str(
            r#"{"planPreset":"max20x","weeklyCeilingUsd":5,"alertThresholds":[80],"doNotDisturb":true,
               "dndSchedule":{"start":"22:00","end":"08:00"},"autostart":false,"dataDir":"x"}"#,
        )
        .unwrap();
        assert_eq!(back.plan_preset, PlanPreset::Max20x);
        assert_eq!(back.dnd_schedule.unwrap().start, "22:00");
        assert_eq!(back.long_task_minutes, 10); // ausente no JSON antigo -> padrão
        assert_eq!(back.scene_mode, SceneMode::Portrait);
        assert_eq!(v["sceneMode"], "portrait");
        let mut j = serde_json::to_value(Settings::defaults(1.0)).unwrap();
        j["sceneMode"] = "avatar3d".into();
        assert_eq!(serde_json::from_value::<Settings>(j.clone()).unwrap().scene_mode, SceneMode::Avatar3d);
        j["sceneMode"] = "hologram".into();
        assert_eq!(serde_json::from_value::<Settings>(j.clone()).unwrap().scene_mode, SceneMode::Portrait);
        assert_eq!(v["longTaskMinutes"], 10);
        assert_eq!(back.ascii_mode, AsciiMode::On); // ausente no JSON antigo -> on
        assert_eq!(v["asciiMode"], "on");
        j["asciiMode"] = "introOnly".into();
        assert_eq!(serde_json::from_value::<Settings>(j.clone()).unwrap().ascii_mode, AsciiMode::IntroOnly);
        j["asciiMode"] = "off".into();
        assert_eq!(serde_json::from_value::<Settings>(j.clone()).unwrap().ascii_mode, AsciiMode::Off);
        j["asciiMode"] = "matrix".into();
        assert_eq!(serde_json::from_value::<Settings>(j).unwrap().ascii_mode, AsciiMode::On);
    }

    #[test]
    fn voice_settings_defaults_and_old_json() {
        let v = serde_json::to_value(Settings::defaults(1.0)).unwrap();
        assert_eq!(v["voiceEnabled"], true);
        assert_eq!(v["voiceEngine"], "auto");
        assert_eq!(v["voiceName"], "pt-BR-FranciscaNeural");
        assert_eq!(v["voiceVolume"], 0.8);
        assert_eq!(v["voiceRate"], -5);
        assert_eq!(v["voicePitch"], -5);
        assert_eq!(v["voiceMuted"], false);
        assert_eq!(v["voiceSpeakOn"], serde_json::json!({"alerts":true,"opening":true,"chat":true,"longTask":true}));
        // JSON antigo, sem nenhum campo de voz
        let old: Settings = serde_json::from_str(
            r#"{"planPreset":"pro","weeklyCeilingUsd":5,"alertThresholds":[80],"doNotDisturb":false,
               "dndSchedule":null,"autostart":false}"#,
        )
        .unwrap();
        let d = Settings::defaults(5.0);
        assert_eq!(old.voice_enabled, d.voice_enabled);
        assert_eq!(old.voice_engine, nyx_voice::EnginePref::Auto);
        assert_eq!(old.voice_name, d.voice_name);
        assert_eq!(old.voice_volume, 0.8);
        assert_eq!((old.voice_rate, old.voice_pitch), (-5, -5));
        assert_eq!(old.voice_speak_on, VoiceSpeakOn::default());
        assert!(!old.voice_muted);
        // valores desconhecidos / parciais
        let mut j = v.clone();
        j["voiceEngine"] = "espeak".into();
        j["voiceSpeakOn"] = serde_json::json!({"chat": false});
        let s: Settings = serde_json::from_value(j).unwrap();
        assert_eq!(s.voice_engine, nyx_voice::EnginePref::Auto);
        assert!(!s.voice_speak_on.chat && s.voice_speak_on.alerts && s.voice_speak_on.long_task);
        let ev = SpeechEvent {
            id: "a".into(),
            kind: SpeechKind::LongTask,
            text: "x".into(),
            engine: nyx_voice::Engine::Windows,
            duration_ms: 10,
            envelope: vec![0.5],
        };
        let e = serde_json::to_value(ev).unwrap();
        assert_eq!(e["kind"], "longTask");
        assert_eq!(e["engine"], "windows");
        assert_eq!(e["durationMs"], 10);
    }
}
