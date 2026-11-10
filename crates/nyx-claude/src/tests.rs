use super::*;
use crate::settings_merge::*;
use serde_json::json;
use std::io::Write;

const FIXTURE: &str = include_str!("../tests/fixtures/transcript.jsonl");

// ---------- normalize_hook ----------

#[test]
fn stop_is_task_done() {
    let p = json!({"session_id":"s1","transcript_path":"C:/t.jsonl","cwd":"C:\\Users\\x\\projeto demo",
        "hook_event_name":"Stop","last_assistant_message":"Pronto."});
    let e = normalize_hook("Stop", &p);
    assert_eq!(e.kind, EventKind::TaskDone);
    assert_eq!(e.title, "Claude Code concluiu");
    assert_eq!(e.session_id, "s1");
    assert_eq!(e.detail.as_deref(), Some("Pronto."));
    assert_eq!(e.transcript_path.as_deref(), Some("C:/t.jsonl"));
    assert_eq!(e.raw_hook, "Stop");
}

#[test]
fn notification_needs_input_with_folder() {
    let p = json!({"session_id":"s1","cwd":"C:\\Users\\x\\projeto demo","message":"Claude precisa de permissão"});
    let e = normalize_hook("Notification", &p);
    assert_eq!(e.kind, EventKind::NeedsInput);
    assert_eq!(e.title, "Dev, o Claude Code em projeto demo quer uma resposta sua.");
    assert_eq!(e.detail.as_deref(), Some("Claude precisa de permissão"));
    assert!(!e.title.contains('!'));
}

#[test]
fn notification_benign_types_do_not_alert() {
    let p = json!({"session_id":"s","cwd":"/a/b","notification_type":"auth_success","message":"ok"});
    assert_eq!(normalize_hook("Notification", &p).kind, EventKind::ToolUse);
}

#[test]
fn other_hooks() {
    let p = json!({"session_id":"s","cwd":"C:/a/b","tool_name":"Bash","source":"startup","reason":"other"});
    let e = normalize_hook("PreToolUse", &p);
    assert_eq!((e.kind, e.detail.as_deref()), (EventKind::ToolUse, Some("Bash")));
    assert_eq!(normalize_hook("SessionStart", &p).kind, EventKind::SessionStart);
    assert_eq!(normalize_hook("SessionEnd", &p).kind, EventKind::SessionEnd);
    assert_eq!(normalize_hook("UserPromptSubmit", &p).kind, EventKind::PromptSubmitted);
    // a subagent finishing must not look like the session finishing
    assert_eq!(normalize_hook("SubagentStop", &p).kind, EventKind::ToolUse);
}

#[test]
fn unknown_or_garbage_does_not_panic() {
    let e = normalize_hook("Weird", &json!(null));
    assert_eq!(e.kind, EventKind::ToolUse);
    assert_eq!(e.session_id, "");
    let e = normalize_hook("Stop", &json!([1, 2, 3]));
    assert_eq!(e.kind, EventKind::TaskDone);
    let e = normalize_hook("", &json!({"hook_event_name":"Stop"}));
    assert_eq!(e.raw_hook, "Stop");
}

#[test]
fn folder_names() {
    assert_eq!(folder_name("C:\\a\\b\\"), "b");
    assert_eq!(folder_name("/home/x/proj"), "proj");
    assert_eq!(folder_name(""), "");
}

// ---------- parse_line / read_from ----------

#[test]
fn parse_line_basics() {
    let line = FIXTURE.lines().nth(2).unwrap();
    let r = parse_line(line).unwrap();
    assert_eq!(r.message_id, "msg_A");
    assert_eq!(r.session_id, "sess-1");
    assert_eq!(r.model, "claude-opus-5-5");
    assert_eq!((r.input, r.output, r.cache_write, r.cache_read), (3, 10, 1000, 5000));
    assert_eq!(r.cwd.as_deref(), Some("C:\\proj\\demo"));
}

#[test]
fn parse_line_rejects_noise() {
    assert!(parse_line("").is_none());
    assert!(parse_line("not json").is_none());
    assert!(parse_line("{\"type\":\"user\",\"usage\":1}").is_none());
    assert!(parse_line(FIXTURE.lines().nth(6).unwrap()).is_none()); // synthetic
    assert!(parse_line(FIXTURE.lines().nth(7).unwrap()).is_none()); // no id
}

#[test]
fn parse_line_tolerates_bom_and_crlf() {
    let line = format!("\u{feff}{}\r", FIXTURE.lines().nth(2).unwrap());
    assert!(parse_line(&line).is_some());
}

fn write_fixture(dir: &Path) -> PathBuf {
    let p = dir.join("t.jsonl");
    std::fs::write(&p, FIXTURE).unwrap();
    p
}

#[test]
fn read_from_dedupes_and_keeps_largest_output() {
    let d = tempfile::tempdir().unwrap();
    let p = write_fixture(d.path());
    let (recs, off) = read_from(&p, 0).unwrap();
    assert_eq!(off, FIXTURE.len() as u64);
    assert_eq!(recs.len(), 2);
    let a = recs.iter().find(|r| r.message_id == "msg_A").unwrap();
    assert_eq!(a.output, 250);
    assert_eq!(recs[1].message_id, "msg_B");
    assert_eq!(recs[1].cache_read, 0);
}

#[test]
fn read_from_only_advances_over_complete_lines() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("t.jsonl");
    let line = FIXTURE.lines().nth(2).unwrap();
    let half = &FIXTURE.lines().nth(5).unwrap()[..40];
    std::fs::write(&p, format!("{line}\n{half}")).unwrap();
    let (recs, off) = read_from(&p, 0).unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(off, line.len() as u64 + 1);
    // the writer finishes the line later: resume from the saved offset
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    let full = FIXTURE.lines().nth(5).unwrap();
    write!(f, "{}\n", &full[40..]).unwrap();
    let (recs, off2) = read_from(&p, off).unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].message_id, "msg_B");
    assert_eq!(off2, std::fs::metadata(&p).unwrap().len());
    // nothing new
    let (recs, off3) = read_from(&p, off2).unwrap();
    assert!(recs.is_empty());
    assert_eq!(off3, off2);
}

#[test]
fn read_from_resets_when_file_shrinks_and_handles_bom_file() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("t.jsonl");
    let line = FIXTURE.lines().nth(2).unwrap();
    std::fs::write(&p, format!("\u{feff}{line}\n")).unwrap();
    let (recs, _) = read_from(&p, 9999).unwrap();
    assert_eq!(recs.len(), 1);
}

#[test]
fn read_from_survives_invalid_utf8() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("t.jsonl");
    let mut bytes = vec![0xff, 0xfe, b'\n'];
    bytes.extend_from_slice(FIXTURE.lines().nth(2).unwrap().as_bytes());
    bytes.push(b'\n');
    std::fs::write(&p, bytes).unwrap();
    assert_eq!(read_from(&p, 0).unwrap().0.len(), 1);
}

// ---------- cost ----------

fn rec(model: &str) -> UsageRecord {
    UsageRecord {
        message_id: "m".into(),
        session_id: "s".into(),
        cwd: None,
        model: model.into(),
        ts: chrono::Utc::now(),
        input: 1_000_000,
        output: 1_000_000,
        cache_write: 1_000_000,
        cache_read: 1_000_000,
    }
}

#[test]
fn cost_by_family() {
    let c = |m: &str| estimate_cost_usd(&rec(m));
    assert!((c("claude-opus-5-5") - (4.0 + 20.0 + 5.0 + 0.2)).abs() < 1e-9);
    assert!((c("claude-opus-4-8") - (5.0 + 25.0 + 6.25 + 0.5)).abs() < 1e-9);
    assert!((c("claude-sonnet-5-5") - (2.0 + 10.0 + 2.5 + 0.2)).abs() < 1e-9);
    assert!((c("claude-sonnet-4-6") - (3.0 + 15.0 + 3.75 + 0.3)).abs() < 1e-9);
    assert!((c("claude-haiku-4-5-20251001") - (1.0 + 5.0 + 1.25 + 0.1)).abs() < 1e-9);
    assert!((c("claude-fable-5-1") - (10.0 + 50.0 + 12.5 + 0.25)).abs() < 1e-9);
    assert!((c("claude-opus-4-1-20250805") - (15.0 + 75.0 + 18.75 + 1.5)).abs() < 1e-9);
}

#[test]
fn unknown_model_priced_as_sonnet_55() {
    let (pr, known) = price_for("gpt-whatever");
    assert!(!known);
    assert_eq!(pr, price_for("claude-sonnet-5-5").0);
    assert!(estimate_cost_usd(&rec("gpt-whatever")) > 0.0);
}

#[test]
fn ceilings_are_ordered_and_positive() {
    let (a, b, c) = (
        default_weekly_ceiling_usd("pro"),
        default_weekly_ceiling_usd("max5x"),
        default_weekly_ceiling_usd("max20x"),
    );
    assert!(a > 0.0 && a < b && b < c);
    assert_eq!(default_weekly_ceiling_usd("???"), b);
}

// ---------- settings_merge ----------

const ORCA: &str = "C:/Users/dev/.orca/agent-hooks/orca-agent-hook.exe claude";

fn orca_settings() -> serde_json::Value {
    json!({
        "model": "opus",
        "permissions": {"allow": ["Bash(ls)"]},
        "hooks": {
            "Stop": [{"hooks":[{"type":"command","command":ORCA,"timeout":10}]}],
            "PermissionRequest": [{"matcher":"*","hooks":[{"type":"command","command":ORCA,"timeout":10}]}],
            "PreToolUse": [{"matcher":"*","hooks":[{"type":"command","command":ORCA,"timeout":10}]}],
        }
    })
}

#[test]
fn install_adds_without_touching_orca() {
    let before = orca_settings();
    let after = install(before.clone(), "C:\\Program Files\\Nyx\\nyx-hook.exe");
    assert_eq!(after["model"], before["model"]);
    assert_eq!(after["permissions"], before["permissions"]);
    assert_eq!(after["hooks"]["PermissionRequest"], before["hooks"]["PermissionRequest"]);
    // Orca entry is still first in Stop/PreToolUse
    assert_eq!(after["hooks"]["Stop"][0], before["hooks"]["Stop"][0]);
    assert_eq!(after["hooks"]["PreToolUse"][0], before["hooks"]["PreToolUse"][0]);
    for ev in NYX_HOOK_EVENTS {
        let groups = after["hooks"][ev].as_array().unwrap();
        let nyx: Vec<_> = groups
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap().iter())
            .filter(|h| is_nyx_command(h["command"].as_str().unwrap()))
            .collect();
        assert_eq!(nyx.len(), 1, "{ev}");
        assert_eq!(
            nyx[0]["command"],
            format!("\"C:/Program Files/Nyx/nyx-hook.exe\" event {ev}")
        );
        assert_eq!(nyx[0]["type"], "command");
        assert!(nyx[0]["timeout"].as_u64().unwrap() > 0);
    }
    assert!(is_installed(&after));
    assert!(!is_installed(&before));
}

#[test]
fn install_is_idempotent_and_refreshes_path() {
    let once = install(orca_settings(), "C:/a/nyx-hook.exe");
    assert_eq!(install(once.clone(), "C:/a/nyx-hook.exe"), once);
    let moved = install(once, "D:/b/nyx-hook.exe");
    let s = serde_json::to_string(&moved).unwrap();
    assert!(!s.contains("C:/a/nyx-hook.exe"));
    assert_eq!(s.matches("D:/b/nyx-hook.exe").count(), 7);
}

#[test]
fn uninstall_restores_original() {
    let before = orca_settings();
    let back = uninstall(install(before.clone(), "C:/a/nyx-hook.exe"));
    assert_eq!(back, before);
}

#[test]
fn install_on_empty_and_uninstall_to_empty() {
    let s = install(json!({}), "C:/a/nyx-hook.exe");
    assert_eq!(s["hooks"].as_object().unwrap().len(), 7);
    assert_eq!(uninstall(s), json!({}));
    assert_eq!(uninstall(json!({"x": 1})), json!({"x": 1}));
}

#[test]
fn install_keeps_weird_shapes() {
    let weird = json!({"hooks": "nope"});
    assert_eq!(install(weird.clone(), "x/nyx-hook.exe"), weird);
}
