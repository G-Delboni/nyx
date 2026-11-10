//! Agregação de uso (sessão / hoje / semana), alertas de teto e export CSV.
//! Funções puras sobre `UsageRow`; o acesso ao banco fica em `core`.

use crate::db::UsageRow;
use crate::model::{ModelUsage, ProjectContext, TokenCounts, UsageRange, UsageSummary};
use crate::sessions::context_for;
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

fn rfc(ts: &DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Início de "hoje" (00:00 no fuso local) em UTC.
pub fn start_of_today(now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&Local);
    let midnight = local.date_naive().and_hms_opt(0, 0, 0).unwrap();
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(now - Duration::hours(24))
}

/// Semana = 7 dias corridos até agora.
pub fn start_of_week(now: DateTime<Utc>) -> DateTime<Utc> {
    now - Duration::days(7)
}

/// Agrega as linhas (já filtradas pelo intervalo). Filtra por contexto se `context_id` vier.
#[allow(clippy::too_many_arguments)]
pub fn summarize(
    rows: &[UsageRow],
    range: UsageRange,
    context_id: Option<&str>,
    contexts: &[ProjectContext],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    ceiling_usd: Option<f64>,
) -> UsageSummary {
    let mut tokens = TokenCounts::default();
    let mut cost = 0.0;
    let mut messages = 0u64;
    let mut sessions = HashSet::new();
    let mut by_model: BTreeMap<String, ModelUsage> = BTreeMap::new();
    for r in rows {
        if let Some(cid) = context_id {
            if context_for(r.cwd.as_deref(), contexts).as_deref() != Some(cid) {
                continue;
            }
        }
        tokens.add(&r.tokens);
        cost += r.cost_usd;
        messages += 1;
        sessions.insert(r.session_id.as_str());
        let m = by_model.entry(r.model.clone()).or_insert_with(|| ModelUsage {
            model: r.model.clone(),
            tokens: TokenCounts::default(),
            cost_usd: 0.0,
            messages: 0,
        });
        m.tokens.add(&r.tokens);
        m.cost_usd += r.cost_usd;
        m.messages += 1;
    }
    let mut by_model: Vec<ModelUsage> = by_model.into_values().collect();
    by_model.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));
    let ceiling = if range == UsageRange::Week { ceiling_usd.filter(|c| *c > 0.0) } else { None };
    UsageSummary {
        range,
        context_id: context_id.map(str::to_string),
        from: rfc(&from),
        to: rfc(&to),
        tokens,
        cost_usd: cost,
        sessions: sessions.len() as u64,
        messages,
        by_model,
        ceiling_usd: ceiling,
        percent: ceiling.map(|c| cost / c * 100.0),
    }
}

/// Registro dos alertas já disparados: uma vez por semana (ISO, fuso local) e por limiar.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlertState {
    pub week: String,
    pub fired: Vec<f64>,
}

pub fn week_key(now: DateTime<Utc>) -> String {
    let w = now.with_timezone(&Local).iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

/// Limiares a disparar agora; atualiza `state` (que deve ser persistido depois).
pub fn thresholds_to_fire(percent: f64, thresholds: &[f64], state: &mut AlertState, week: &str) -> Vec<f64> {
    if state.week != week {
        *state = AlertState { week: week.to_string(), fired: vec![] };
    }
    let mut out = vec![];
    let mut sorted: Vec<f64> = thresholds.iter().copied().filter(|t| *t > 0.0).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for t in sorted {
        if percent >= t && !state.fired.iter().any(|f| (f - t).abs() < 1e-9) {
            state.fired.push(t);
            out.push(t);
        }
    }
    out
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Uma linha por mensagem do intervalo (filtrada por contexto se vier).
pub fn to_csv(rows: &[UsageRow], context_id: Option<&str>, contexts: &[ProjectContext]) -> String {
    let mut out = String::from(
        "timestamp,session_id,context_id,cwd,model,input_tokens,output_tokens,cache_write_tokens,cache_read_tokens,cost_usd_estimate\n",
    );
    for r in rows {
        let ctx = context_for(r.cwd.as_deref(), contexts);
        if context_id.is_some() && ctx.as_deref() != context_id {
            continue;
        }
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{:.6}\n",
            rfc(&r.ts),
            csv_field(&r.session_id),
            csv_field(ctx.as_deref().unwrap_or("")),
            csv_field(r.cwd.as_deref().unwrap_or("")),
            csv_field(&r.model),
            r.tokens.input,
            r.tokens.output,
            r.tokens.cache_write,
            r.tokens.cache_read,
            r.cost_usd
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn row(id: &str, s: &str, cwd: &str, model: &str, cost: f64) -> UsageRow {
        UsageRow {
            message_id: id.into(),
            session_id: s.into(),
            cwd: Some(cwd.into()),
            model: model.into(),
            ts: Utc.with_ymd_and_hms(2026, 11, 7, 3, 0, 0).unwrap(),
            tokens: TokenCounts { input: 10, output: 5, cache_write: 1, cache_read: 2 },
            cost_usd: cost,
        }
    }

    fn ctx() -> Vec<ProjectContext> {
        vec![ProjectContext {
            id: "cd".into(),
            name: "Projeto Demo".into(),
            root_path: "C:\\Users\\x\\projeto demo".into(),
            instructions: String::new(),
            color: None,
            created_at: String::new(),
            ..Default::default()
        }]
    }

    #[test]
    fn aggregates_by_model_and_context() {
        let rows = vec![
            row("1", "a", "C:/Users/x/projeto demo/ep1", "claude-opus", 2.0),
            row("2", "a", "C:/Users/x/projeto demo", "claude-haiku", 0.5),
            row("3", "b", "C:/Users/x/nyx", "claude-opus", 1.0),
        ];
        let now = Utc::now();
        let s = summarize(&rows, UsageRange::Week, None, &ctx(), now, now, Some(10.0));
        assert_eq!(s.messages, 3);
        assert_eq!(s.sessions, 2);
        assert_eq!(s.tokens.input, 30);
        assert!((s.cost_usd - 3.5).abs() < 1e-9);
        assert_eq!(s.by_model[0].model, "claude-opus");
        assert_eq!(s.by_model[0].messages, 2);
        assert!((s.percent.unwrap() - 35.0).abs() < 1e-9);

        let c = summarize(&rows, UsageRange::Today, Some("cd"), &ctx(), now, now, Some(10.0));
        assert_eq!(c.messages, 2);
        assert_eq!(c.sessions, 1);
        assert_eq!(c.ceiling_usd, None);
        assert_eq!(c.percent, None);
    }

    #[test]
    fn alerts_fire_once_per_week_and_threshold() {
        let mut st = AlertState::default();
        assert_eq!(thresholds_to_fire(50.0, &[80.0, 95.0], &mut st, "2026-W45"), Vec::<f64>::new());
        assert_eq!(thresholds_to_fire(82.0, &[80.0, 95.0], &mut st, "2026-W45"), vec![80.0]);
        assert_eq!(thresholds_to_fire(85.0, &[80.0, 95.0], &mut st, "2026-W45"), Vec::<f64>::new());
        assert_eq!(thresholds_to_fire(99.0, &[95.0, 80.0], &mut st, "2026-W45"), vec![95.0]);
        // semana nova: zera
        assert_eq!(thresholds_to_fire(99.0, &[80.0, 95.0], &mut st, "2026-W46"), vec![80.0, 95.0]);
    }

    #[test]
    fn csv_escapes_and_filters() {
        let rows = vec![row("1", "a", "C:/Users/x/projeto demo", "m,1", 1.0), row("2", "b", "C:/y", "m", 1.0)];
        let all = to_csv(&rows, None, &ctx());
        assert_eq!(all.lines().count(), 3);
        assert!(all.contains("\"m,1\""));
        assert!(all.contains(",cd,"));
        assert_eq!(to_csv(&rows, Some("cd"), &ctx()).lines().count(), 2);
    }

    #[test]
    fn ranges() {
        let now = Utc::now();
        let t = start_of_today(now);
        assert!(t <= now && now - t <= Duration::hours(25));
        assert_eq!(now - start_of_week(now), Duration::days(7));
        assert!(week_key(now).contains("-W"));
    }
}
