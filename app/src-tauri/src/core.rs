//! Lógica do núcleo sobre o banco: aplica eventos de hook, grava uso, agrega,
//! dispara alertas e monta o snapshot. Sem Tauri aqui, para dar para testar
//! com dados falsos (sem chamar as funções do `nyx-claude`).

use crate::db::{Db, NewEvent, SessionRow, UsageRow};
use crate::model::*;
use crate::sessions::{self, context_for, folder_name};
use crate::toast::{self, Toast};
use crate::usage::{self, AlertState};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use nyx_claude::{NormalizedEvent, UsageRecord};
use std::path::{Path, PathBuf};

pub const AGENT_CLAUDE: &str = "claude-code";
const SETTINGS_KEY: &str = "settings";
const ALERTS_KEY: &str = "usage_alerts";
/// Cards de agentes: sessões com atividade nas últimas 24 h, no máximo 20.
const AGENTS_WINDOW_HOURS: i64 = 24;
const AGENTS_LIMIT: u32 = 20;

#[derive(Debug, Clone, Default)]
pub struct Applied {
    /// Evento gravado (None para eventos que só mudam status, como PreToolUse).
    pub event: Option<NyxEvent>,
    pub toast: Option<Toast>,
    pub session_changed: bool,
}

/// Aplica um evento normalizado: atualiza a sessão, grava o evento e prepara o toast.
/// Um `Stop` depois de `long_task_minutes` seguidos em `running` vira toast de tarefa
/// longa (prioridade média) em vez do toast alto normal.
pub fn apply_event(db: &Db, ne: &NormalizedEvent, now: DateTime<Utc>, long_task_minutes: u32) -> Result<Applied> {
    let kind = EventKind::from_claude(ne.kind);
    let contexts = db.list_contexts()?;
    let mut out = Applied::default();

    let existing = if ne.session_id.is_empty() { None } else { db.get_session(&ne.session_id)? };
    let cwd = ne.cwd.clone().or_else(|| existing.as_ref().and_then(|s| s.cwd.clone()));
    // Desde quando estava rodando sem interrupção (só conta se ainda estava `running`).
    let prev_running_since = existing
        .as_ref()
        .filter(|s| s.status == SessionStatus::Running)
        .and_then(|s| s.running_since);
    let long_task = match (kind, prev_running_since) {
        (EventKind::TaskDone, Some(since)) if long_task_minutes > 0 => {
            let mins = (now - since).num_minutes();
            (mins >= long_task_minutes as i64).then_some(mins)
        }
        _ => None,
    };

    if let (false, Some(status)) = (ne.session_id.is_empty(), sessions::status_for(kind)) {
        let last_message = match kind {
            EventKind::NeedsInput | EventKind::Error | EventKind::TaskDone => ne.detail.clone(),
            _ => None,
        }
        .or_else(|| existing.as_ref().and_then(|s| s.last_message.clone()));
        let row = SessionRow {
            session_id: ne.session_id.clone(),
            agent: AGENT_CLAUDE.into(),
            cwd: cwd.clone(),
            status,
            last_message,
            transcript_path: ne
                .transcript_path
                .clone()
                .or_else(|| existing.as_ref().and_then(|s| s.transcript_path.clone())),
            started_at: existing.as_ref().map(|s| s.started_at).unwrap_or(now),
            updated_at: now,
            running_since: (status == SessionStatus::Running).then(|| prev_running_since.unwrap_or(now)),
        };
        db.upsert_session(&row)?;
        out.session_changed = true;
    }

    if sessions::should_store(kind) {
        let folder = folder_name(cwd.as_deref());
        let mut t = match long_task {
            Some(mins) => Some(toast::for_long_task(folder.as_deref(), mins)),
            None => toast::for_event(kind, folder.as_deref(), ne.detail.as_deref()),
        };
        let title = if ne.title.trim().is_empty() {
            t.as_ref().map(|t| t.title.clone()).unwrap_or_else(|| kind.as_str().to_string())
        } else {
            ne.title.clone()
        };
        let ev = db.insert_event(&NewEvent {
            kind,
            priority: t.as_ref().map(|t| t.priority).unwrap_or_else(|| sessions::priority_for(kind)),
            session_id: (!ne.session_id.is_empty()).then(|| ne.session_id.clone()),
            context_id: context_for(cwd.as_deref(), &contexts),
            title,
            detail: match long_task {
                Some(_) => t.as_ref().map(|t| t.body.clone()),
                None => ne.detail.clone(),
            },
            created_at: now,
            raw_hook: Some(ne.raw_hook.clone()),
        })?;
        if let Some(t) = t.as_mut() {
            t.event_id = Some(ev.id);
        }
        out.toast = t;
        out.event = Some(ev);
    }
    Ok(out)
}

/// Grava registros de uso (dedup por `message_id`). Sessões com uso recente que
/// ainda não chegaram por hook (hooks não instalados) aparecem como `running`.
pub fn ingest_usage(
    db: &mut Db,
    records: Vec<UsageRecord>,
    cost: impl Fn(&UsageRecord) -> f64,
    now: DateTime<Utc>,
) -> Result<usize> {
    if records.is_empty() {
        return Ok(0);
    }
    let with_cost: Vec<(UsageRecord, f64)> = records.into_iter().map(|r| {
        let c = cost(&r);
        (r, c)
    }).collect();
    let inserted = db.insert_usage(&with_cost)?;

    let recent = now - Duration::minutes(sessions::IDLE_AFTER_MINUTES);
    let mut latest: std::collections::HashMap<&str, &UsageRecord> = Default::default();
    for (r, _) in &with_cost {
        if r.ts > recent && !r.session_id.is_empty() {
            let e = latest.entry(r.session_id.as_str()).or_insert(r);
            if r.ts > e.ts {
                *e = r;
            }
        }
    }
    for (sid, r) in latest {
        match db.get_session(sid)? {
            None => db.upsert_session(&SessionRow {
                session_id: sid.to_string(),
                agent: AGENT_CLAUDE.into(),
                cwd: r.cwd.clone(),
                status: SessionStatus::Running,
                last_message: None,
                transcript_path: None,
                started_at: r.ts,
                updated_at: r.ts,
                running_since: Some(r.ts),
            })?,
            Some(s) if s.status == SessionStatus::Idle && r.ts > s.updated_at => db.upsert_session(&SessionRow {
                status: SessionStatus::Running,
                updated_at: r.ts,
                running_since: Some(r.ts),
                ..s
            })?,
            _ => {}
        }
    }
    Ok(inserted)
}

/// Sessões paradas há 30 min viram `idle`. Devolve quantas mudaram.
pub fn sweep_idle(db: &Db, now: DateTime<Utc>) -> Result<usize> {
    db.mark_idle(now - Duration::minutes(sessions::IDLE_AFTER_MINUTES), sessions::IDLE_FROM)
}

pub fn usage_summary(
    db: &Db,
    range: UsageRange,
    context_id: Option<&str>,
    settings: &Settings,
    now: DateTime<Utc>,
) -> Result<UsageSummary> {
    let contexts = db.list_contexts()?;
    let (rows, from) = usage_rows(db, range, context_id, &contexts, now)?;
    Ok(usage::summarize(&rows, range, context_id, &contexts, from, now, Some(settings.weekly_ceiling_usd)))
}

fn usage_rows(
    db: &Db,
    range: UsageRange,
    context_id: Option<&str>,
    contexts: &[ProjectContext],
    now: DateTime<Utc>,
) -> Result<(Vec<UsageRow>, DateTime<Utc>)> {
    Ok(match range {
        UsageRange::Today => {
            let from = usage::start_of_today(now);
            (db.usage_between(from, now + Duration::seconds(1))?, from)
        }
        UsageRange::Week => {
            let from = usage::start_of_week(now);
            (db.usage_between(from, now + Duration::seconds(1))?, from)
        }
        UsageRange::Session => match current_session(db, context_id, contexts)? {
            Some(sid) => {
                let rows = db.usage_for_session(&sid)?;
                let from = rows.first().map(|r| r.ts).unwrap_or(now);
                (rows, from)
            }
            None => (vec![], now),
        },
    })
}

/// "Sessão atual" = a última sessão com evento (no contexto, se vier);
/// sem sessões de hook, a última com uso nos transcripts.
fn current_session(db: &Db, context_id: Option<&str>, contexts: &[ProjectContext]) -> Result<Option<String>> {
    for s in db.list_sessions(50)? {
        if context_id.is_none() || context_for(s.cwd.as_deref(), contexts).as_deref() == context_id {
            return Ok(Some(s.session_id));
        }
    }
    if context_id.is_none() {
        return db.latest_usage_session();
    }
    Ok(None)
}

/// Confere o uso semanal contra o teto e grava/devolve os alertas novos.
pub fn check_usage_alerts(db: &Db, settings: &Settings, now: DateTime<Utc>) -> Result<Vec<(NyxEvent, Toast)>> {
    let week = usage_summary(db, UsageRange::Week, None, settings, now)?;
    let (Some(percent), Some(ceiling)) = (week.percent, week.ceiling_usd) else { return Ok(vec![]) };
    let mut state: AlertState = db.get_setting(ALERTS_KEY)?.unwrap_or_default();
    let before = state.clone();
    let fire = usage::thresholds_to_fire(percent, &settings.alert_thresholds, &mut state, &usage::week_key(now));
    if state != before {
        db.set_setting(ALERTS_KEY, &state)?;
    }
    let mut out = vec![];
    for t in fire {
        let mut toast = toast::for_usage(t, percent, week.cost_usd, ceiling);
        let ev = db.insert_event(&NewEvent {
            kind: EventKind::UsageThreshold,
            priority: Priority::Medium,
            session_id: None,
            context_id: None,
            title: toast.title.clone(),
            detail: Some(toast.body.clone()),
            created_at: now,
            raw_hook: None,
        })?;
        toast.event_id = Some(ev.id);
        out.push((ev, toast));
    }
    Ok(out)
}

pub fn agents(db: &Db, now: DateTime<Utc>) -> Result<Vec<AgentSession>> {
    let contexts = db.list_contexts()?;
    let cutoff = now - Duration::hours(AGENTS_WINDOW_HOURS);
    let mut out = vec![];
    for s in db.list_sessions(AGENTS_LIMIT)? {
        if s.updated_at < cutoff {
            break;
        }
        let (tokens, cost_usd) = db.session_totals(&s.session_id)?;
        out.push(AgentSession {
            folder_name: folder_name(s.cwd.as_deref()),
            context_id: context_for(s.cwd.as_deref(), &contexts),
            session_id: s.session_id,
            agent: s.agent,
            cwd: s.cwd,
            status: s.status,
            started_at: s.started_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            updated_at: s.updated_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            last_message: s.last_message,
            tokens,
            cost_usd,
        });
    }
    Ok(out)
}

pub fn nyx_status(agents: &[AgentSession], week: &UsageSummary, settings: &Settings) -> NyxStatus {
    let top = settings.alert_thresholds.iter().copied().fold(f64::NAN, f64::max);
    let over = matches!((week.percent, top.is_nan()), (Some(p), false) if p >= top);
    if over || agents.iter().any(|a| matches!(a.status, SessionStatus::Waiting | SessionStatus::Error)) {
        NyxStatus::Alert
    } else if agents.iter().any(|a| a.status == SessionStatus::Running) {
        NyxStatus::Analyzing
    } else {
        NyxStatus::Active
    }
}

pub fn snapshot(db: &Db, settings: &Settings, now: DateTime<Utc>) -> Result<Snapshot> {
    let agents = agents(db, now)?;
    let week = usage_summary(db, UsageRange::Week, None, settings, now)?;
    Ok(Snapshot {
        status: nyx_status(&agents, &week, settings),
        active_delegations: 0, // preenchido por quem tem o motor de delegação
        agents,
        unread_events: db.unread_count()?,
        week_usage: week,
        settings: settings.clone(),
        contexts: db.list_contexts()?,
    })
}

// ---------- settings ----------

pub fn load_settings(db: &Db, default_ceiling: impl FnOnce() -> f64, data_dir: &Path) -> Result<Settings> {
    let mut s = match db.get_setting::<Settings>(SETTINGS_KEY)? {
        Some(s) => s,
        None => {
            let s = Settings::defaults(default_ceiling());
            db.set_setting(SETTINGS_KEY, &s)?;
            s
        }
    };
    s.data_dir = data_dir.display().to_string();
    Ok(s)
}

/// Normaliza e grava. `dataDir` é só informativo e não é persistido.
pub fn save_settings(db: &Db, mut s: Settings, data_dir: &Path) -> Result<Settings> {
    s.alert_thresholds.retain(|t| t.is_finite() && *t > 0.0);
    s.alert_thresholds.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s.alert_thresholds.dedup();
    if !s.weekly_ceiling_usd.is_finite() || s.weekly_ceiling_usd < 0.0 {
        s.weekly_ceiling_usd = 0.0;
    }
    s.long_task_minutes = s.long_task_minutes.clamp(1, 24 * 60);
    s.voice_volume = if s.voice_volume.is_finite() { s.voice_volume.clamp(0.0, 1.0) } else { DEFAULT_VOICE_VOLUME };
    s.voice_rate = s.voice_rate.clamp(-50, 50);
    s.voice_pitch = s.voice_pitch.clamp(-50, 50);
    s.voice_name = valid_voice_name(&s.voice_name).unwrap_or_else(|| nyx_voice::DEFAULT_VOICE.to_string());
    s.data_dir = String::new();
    db.set_setting(SETTINGS_KEY, &s)?;
    s.data_dir = data_dir.display().to_string();
    Ok(s)
}

/// Nome de voz da Azure (`pt-BR-FranciscaNeural`): letras, dígitos e hífen, até 64.
/// Vai para dentro do SSML, então nada além disso passa.
pub fn valid_voice_name(name: &str) -> Option<String> {
    let n = name.trim();
    let ok = !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then(|| n.to_string())
}

// ---------- contexts ----------

pub fn save_context(db: &Db, mut c: ProjectContext, now: DateTime<Utc>) -> Result<ProjectContext> {
    anyhow::ensure!(!c.name.trim().is_empty(), "o contexto precisa de um nome");
    anyhow::ensure!(!c.root_path.trim().is_empty(), "o contexto precisa de uma pasta raiz");
    if c.id.trim().is_empty() {
        c.id = new_id();
    }
    if c.created_at.trim().is_empty() {
        c.created_at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
    c.name = c.name.trim().to_string();
    c.root_path = c.root_path.trim().to_string();
    db.save_context(&c)?;
    Ok(c)
}

pub fn new_id() -> String {
    let b: [u8; 16] = rand::random();
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

// ---------- export ----------

pub fn export_csv(
    db: &Db,
    range: UsageRange,
    context_id: Option<&str>,
    now: DateTime<Utc>,
    dir: &Path,
) -> Result<PathBuf> {
    let contexts = db.list_contexts()?;
    let (rows, _) = usage_rows(db, range, context_id, &contexts, now)?;
    let csv = usage::to_csv(&rows, context_id, &contexts);
    std::fs::create_dir_all(dir)?;
    let stamp = now.with_timezone(&chrono::Local).format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("usage-{}-{stamp}.csv", range.as_str()));
    std::fs::write(&path, csv)?;
    Ok(path)
}

/// `%USERPROFILE%\Documents\Nyx`.
pub fn export_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Nyx")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::rec;
    use chrono::TimeZone;
    use nyx_claude::EventKind as CK;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).unwrap()
    }

    fn ev(kind: CK, sid: &str, cwd: Option<&str>) -> NormalizedEvent {
        NormalizedEvent {
            kind,
            session_id: sid.into(),
            cwd: cwd.map(Into::into),
            transcript_path: None,
            title: match kind {
                CK::TaskDone => "Claude Code concluiu".into(),
                _ => String::new(),
            },
            detail: (kind == CK::NeedsInput).then(|| "Claude needs your permission".into()),
            raw_hook: "{}".into(),
        }
    }

    fn with_ctx(db: &Db) -> ProjectContext {
        save_context(
            db,
            ProjectContext {
                id: String::new(),
                name: "Projeto Demo".into(),
                root_path: "C:\\Users\\dev\\Documents\\projeto demo".into(),
                instructions: String::new(),
                color: None,
                created_at: String::new(),
                ..Default::default()
            },
            now(),
        )
        .unwrap()
    }

    #[test]
    fn session_lifecycle_and_toasts() {
        let db = Db::open_in_memory().unwrap();
        let c = with_ctx(&db);
        assert_eq!(c.id.len(), 36);
        let cwd = Some("c:/users/dev/documents/Projeto Demo/ep1");

        let a = apply_event(&db, &ev(CK::SessionStart, "s1", cwd), now(), 10).unwrap();
        assert!(a.toast.is_none());
        assert_eq!(a.event.unwrap().priority, Priority::Low);

        let a = apply_event(&db, &ev(CK::ToolUse, "s1", None), now(), 10).unwrap();
        assert!(a.event.is_none() && a.session_changed);
        assert_eq!(db.get_session("s1").unwrap().unwrap().status, SessionStatus::Running);

        let a = apply_event(&db, &ev(CK::NeedsInput, "s1", None), now(), 10).unwrap();
        let t = a.toast.unwrap();
        assert_eq!(t.body, "Dev, o Claude Code em ep1 quer uma resposta sua.");
        assert_eq!(t.event_id, a.event.as_ref().map(|e| e.id));
        assert_eq!(a.event.unwrap().context_id.as_deref(), Some(c.id.as_str()));

        let ag = agents(&db, now()).unwrap();
        assert_eq!(ag.len(), 1);
        assert_eq!(ag[0].status, SessionStatus::Waiting);
        assert_eq!(ag[0].folder_name.as_deref(), Some("ep1"));
        assert_eq!(ag[0].last_message.as_deref(), Some("Claude needs your permission"));
        let settings = Settings::defaults(100.0);
        let snap = snapshot(&db, &settings, now()).unwrap();
        assert_eq!(snap.status, NyxStatus::Alert);
        assert_eq!(snap.unread_events, 2);

        let a = apply_event(&db, &ev(CK::TaskDone, "s1", cwd), now() + Duration::minutes(1), 10).unwrap();
        assert_eq!(a.toast.unwrap().title, "Claude Code concluiu");
        assert_eq!(db.get_session("s1").unwrap().unwrap().status, SessionStatus::Done);
        assert_eq!(db.get_session("s1").unwrap().unwrap().started_at, now());

        // 30 min sem evento -> idle
        assert_eq!(sweep_idle(&db, now() + Duration::minutes(20)).unwrap(), 0);
        assert_eq!(sweep_idle(&db, now() + Duration::minutes(32)).unwrap(), 1);
        assert_eq!(db.get_session("s1").unwrap().unwrap().status, SessionStatus::Idle);

        let a = apply_event(&db, &ev(CK::SessionEnd, "s1", None), now() + Duration::minutes(40), 10).unwrap();
        assert!(a.toast.is_none());
        assert_eq!(db.get_session("s1").unwrap().unwrap().status, SessionStatus::Ended);
        assert_eq!(db.list_events(50, Some(&c.id)).unwrap().len(), 4);
    }

    #[test]
    fn long_task_toast() {
        let db = Db::open_in_memory().unwrap();
        let cwd = Some("C:/Users/dev/Documents/nyx");
        let m = |n| now() + Duration::minutes(n);
        // 12 min seguidos rodando (PreToolUse no meio não reinicia a contagem)
        apply_event(&db, &ev(CK::PromptSubmitted, "L", cwd), m(0), 10).unwrap();
        apply_event(&db, &ev(CK::ToolUse, "L", None), m(6), 10).unwrap();
        assert_eq!(db.get_session("L").unwrap().unwrap().running_since, Some(m(0)));
        let a = apply_event(&db, &ev(CK::TaskDone, "L", None), m(12), 10).unwrap();
        let t = a.toast.unwrap();
        assert_eq!(t.priority, Priority::Medium);
        assert_eq!(t.body, "Tarefa longa finalizada em nyx. 12 min.");
        assert_eq!(a.event.as_ref().unwrap().priority, Priority::Medium);
        assert_eq!(db.get_session("L").unwrap().unwrap().running_since, None);

        // curta: toast alto normal
        apply_event(&db, &ev(CK::PromptSubmitted, "L", None), m(20), 10).unwrap();
        let a = apply_event(&db, &ev(CK::TaskDone, "L", None), m(25), 10).unwrap();
        assert_eq!(a.toast.unwrap().priority, Priority::High);

        // interrompida por um pedido (waiting): não é "seguida"
        apply_event(&db, &ev(CK::PromptSubmitted, "L", None), m(30), 10).unwrap();
        apply_event(&db, &ev(CK::NeedsInput, "L", None), m(35), 10).unwrap();
        apply_event(&db, &ev(CK::PromptSubmitted, "L", None), m(36), 10).unwrap();
        let a = apply_event(&db, &ev(CK::TaskDone, "L", None), m(44), 10).unwrap();
        assert_eq!(a.toast.unwrap().priority, Priority::High);

        // limiar configurável
        apply_event(&db, &ev(CK::PromptSubmitted, "L", None), m(50), 10).unwrap();
        let a = apply_event(&db, &ev(CK::TaskDone, "L", None), m(55), 5).unwrap();
        assert_eq!(a.toast.unwrap().body, "Tarefa longa finalizada em nyx. 5 min.");
    }

    #[test]
    fn usage_views_and_alerts() {
        let mut db = Db::open_in_memory().unwrap();
        let c = with_ctx(&db);
        let n = now();
        let recs = vec![
            rec("m1", "s1", Some("C:\\Users\\dev\\Documents\\projeto demo"), "claude-opus-4", n - Duration::minutes(5), 1000),
            rec("m1", "s1", Some("C:\\Users\\dev\\Documents\\projeto demo"), "claude-opus-4", n - Duration::minutes(5), 1000),
            rec("m2", "s2", Some("C:\\Users\\dev\\Documents\\nyx"), "claude-haiku", n - Duration::days(2), 10),
            rec("m3", "s3", None, "claude-haiku", n - Duration::days(9), 10),
        ];
        let inserted = ingest_usage(&mut db, recs, |r| r.input as f64 / 100.0, n).unwrap();
        assert_eq!(inserted, 3);
        // s1 tem uso recente sem hook -> aparece como running
        assert_eq!(db.get_session("s1").unwrap().unwrap().status, SessionStatus::Running);
        assert!(db.get_session("s2").unwrap().is_none());

        let mut settings = Settings::defaults(12.0);
        let week = usage_summary(&db, UsageRange::Week, None, &settings, n).unwrap();
        assert_eq!(week.messages, 2);
        assert!((week.cost_usd - 10.1).abs() < 1e-9);
        assert!((week.percent.unwrap() - 10.1 / 12.0 * 100.0).abs() < 1e-9);
        let wc = usage_summary(&db, UsageRange::Week, Some(&c.id), &settings, n).unwrap();
        assert_eq!(wc.messages, 1);
        let sess = usage_summary(&db, UsageRange::Session, None, &settings, n).unwrap();
        assert_eq!(sess.sessions, 1);
        assert_eq!(sess.tokens.input, 1000);
        assert_eq!(sess.ceiling_usd, None);

        // 84% -> dispara 80 uma vez
        let fired = check_usage_alerts(&db, &settings, n).unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].0.kind, EventKind::UsageThreshold);
        assert_eq!(fired[0].1.priority, Priority::Medium);
        assert!(check_usage_alerts(&db, &settings, n).unwrap().is_empty());
        settings.weekly_ceiling_usd = 10.0;
        assert_eq!(check_usage_alerts(&db, &settings, n).unwrap().len(), 1); // 95
        assert!(check_usage_alerts(&db, &settings, n).unwrap().is_empty());

        let dir = tempfile::tempdir().unwrap();
        let p = export_csv(&db, UsageRange::Week, None, n, dir.path()).unwrap();
        let body = std::fs::read_to_string(&p).unwrap();
        assert_eq!(body.lines().count(), 3);
        assert!(p.file_name().unwrap().to_string_lossy().starts_with("usage-week-"));
    }

    #[test]
    fn settings_defaults_and_save() {
        let db = Db::open_in_memory().unwrap();
        let dir = Path::new("C:/tmp/nyx");
        let s = load_settings(&db, || 42.0, dir).unwrap();
        assert_eq!(s.weekly_ceiling_usd, 42.0);
        assert!(!s.autostart);
        assert_eq!(s.alert_thresholds, vec![80.0, 95.0]);
        let mut s2 = s.clone();
        s2.alert_thresholds = vec![95.0, 80.0, 95.0, -1.0];
        s2.weekly_ceiling_usd = 70.0;
        let saved = save_settings(&db, s2, dir).unwrap();
        assert_eq!(saved.alert_thresholds, vec![80.0, 95.0]);
        let s3 = load_settings(&db, || unreachable!(), dir).unwrap();
        assert_eq!(s3.weekly_ceiling_usd, 70.0);
        assert_eq!(s3.data_dir, dir.display().to_string());
    }

    #[test]
    fn voice_settings_validation() {
        let db = Db::open_in_memory().unwrap();
        let dir = Path::new("C:/tmp/nyx");
        let mut s = load_settings(&db, || 1.0, dir).unwrap();
        s.voice_volume = 3.0;
        s.voice_rate = -900;
        s.voice_pitch = 77;
        s.voice_name = "  pt-BR-ThalitaNeural ".into();
        let saved = save_settings(&db, s.clone(), dir).unwrap();
        assert_eq!(saved.voice_volume, 1.0);
        assert_eq!((saved.voice_rate, saved.voice_pitch), (-50, 50));
        assert_eq!(saved.voice_name, "pt-BR-ThalitaNeural");
        s.voice_volume = f64::NAN;
        s.voice_name = "x\"/><evil>".into();
        let saved = save_settings(&db, s.clone(), dir).unwrap();
        assert_eq!(saved.voice_volume, 0.8);
        assert_eq!(saved.voice_name, "pt-BR-FranciscaNeural");
        s.voice_name = String::new();
        s.voice_volume = -1.0;
        let saved = save_settings(&db, s, dir).unwrap();
        assert_eq!(saved.voice_name, "pt-BR-FranciscaNeural");
        assert_eq!(saved.voice_volume, 0.0);
        assert_eq!(load_settings(&db, || unreachable!(), dir).unwrap().voice_volume, 0.0);
    }
}
