//! End-to-end tests for the nyx-hook binary. Every test uses temporary directories:
//! `NYX_DATA_DIR` for runtime/logs and an explicit `--settings` temp file for install.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn hook_cmd(data_dir: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_nyx-hook"));
    c.env("NYX_DATA_DIR", data_dir);
    c
}

fn run_event(data_dir: &std::path::Path, hook: &str, stdin: &str) -> std::process::Output {
    let mut child = hook_cmd(data_dir)
        .args(["event", hook])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

/// Minimal HTTP server: accepts one request, sends it to the channel, replies 202.
fn fake_server(status_line: &'static str) -> (u16, mpsc::Receiver<(String, Vec<u8>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = s.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                let len = head
                    .lines()
                    .find_map(|l| {
                        let l = l.to_ascii_lowercase();
                        l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if buf.len() >= pos + 4 + len {
                    let body = buf[pos + 4..pos + 4 + len].to_vec();
                    let _ = s.write_all(
                        format!("HTTP/1.1 {status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
                    );
                    let _ = tx.send((head, body));
                    return;
                }
            }
        }
    });
    (port, rx)
}

fn write_runtime(dir: &std::path::Path, port: u16) {
    std::fs::write(
        dir.join("runtime.json"),
        json!({"port": port, "token": "abc123token", "pid": 1}).to_string(),
    )
    .unwrap();
}

#[test]
fn posts_envelope_with_token() {
    let dir = tempfile::tempdir().unwrap();
    let (port, rx) = fake_server("202 Accepted");
    write_runtime(dir.path(), port);
    let out = run_event(dir.path(), "Stop", r#"{"session_id":"s1","cwd":"C:\\p"}"#);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "stdout must stay empty");
    let (head, body) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(head.starts_with("POST /v1/events HTTP/1.1"), "{head}");
    assert!(head.to_ascii_lowercase().contains("x-nyx-token: abc123token"), "{head}");
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["source"], "claude-code");
    assert_eq!(v["hook"], "Stop");
    assert_eq!(v["payload"]["session_id"], "s1");
    assert!(v["received_at"].as_str().unwrap().contains('T'));
    // the log exists and records the 202
    let log = std::fs::read_to_string(dir.path().join("logs").join("hook.log")).unwrap();
    assert!(log.contains("Stop: 202"), "{log}");
}

#[test]
fn silent_success_without_runtime_file() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_event(dir.path(), "Stop", r#"{"session_id":"s1"}"#);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn silent_success_when_server_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    // grab a free port, then close it so nothing listens there
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    write_runtime(dir.path(), port);
    let t = std::time::Instant::now();
    let out = run_event(dir.path(), "Notification", "{}");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    // Budget is 1.5 s; 4 s leaves room for a cold process start on a loaded Windows box.
    assert!(t.elapsed() < Duration::from_millis(4000));
}

#[test]
fn silent_success_on_hung_server_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    write_runtime(dir.path(), listener.local_addr().unwrap().port());
    // accept but never answer
    let _hold = std::thread::spawn(move || {
        let _c = listener.accept();
        std::thread::sleep(Duration::from_secs(5));
    });
    let t = std::time::Instant::now();
    let out = run_event(dir.path(), "Stop", "{}");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(t.elapsed() < Duration::from_millis(4000), "took {:?}", t.elapsed());
}

#[test]
fn garbage_stdin_and_http_error_still_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let (port, rx) = fake_server("401 Unauthorized");
    write_runtime(dir.path(), port);
    let out = run_event(dir.path(), "Stop", "this is not json");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let (_, body) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["payload"], Value::Null);
}

#[test]
fn log_rotates_at_one_megabyte() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("hook.log"), vec![b'x'; 1024 * 1024 + 10]).unwrap();
    run_event(dir.path(), "Stop", "{}");
    assert!(logs.join("hook.log.1").exists());
    assert!(std::fs::metadata(logs.join("hook.log")).unwrap().len() < 1000);
}

// ---------- install / uninstall (temporary settings files only) ----------

const ORCA: &str = "C:/Users/dev/.orca/agent-hooks/orca-agent-hook.exe claude";

fn sample_settings() -> Value {
    json!({
        "model": "opus",
        "hooks": {
            "Stop": [{"hooks":[{"type":"command","command":ORCA,"timeout":10}]}],
            "PermissionRequest": [{"matcher":"*","hooks":[{"type":"command","command":ORCA,"timeout":10}]}]
        }
    })
}

fn settings_cmd(dir: &std::path::Path, sub: &str, path: &std::path::Path, extra: &[&str]) -> std::process::Output {
    hook_cmd(dir).arg(sub).arg("--settings").arg(path).args(extra).output().unwrap()
}

fn backups(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().contains("nyx-backup-"))
        .collect()
}

#[test]
fn install_dry_run_does_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("settings.json");
    let original = serde_json::to_string_pretty(&sample_settings()).unwrap();
    std::fs::write(&f, &original).unwrap();
    let out = settings_cmd(dir.path(), "install", &f, &["--dry-run"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("nyx-hook"));
    assert_eq!(std::fs::read_to_string(&f).unwrap(), original);
    assert!(backups(dir.path()).is_empty());
}

#[test]
fn install_then_uninstall_roundtrip_with_backup() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("settings.json");
    let original = sample_settings();
    std::fs::write(&f, serde_json::to_string_pretty(&original).unwrap()).unwrap();

    let out = settings_cmd(dir.path(), "install", &f, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(backups(dir.path()).len(), 1);
    let installed: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    // user's key order preserved (serde_json preserve_order)
    let keys: Vec<_> = installed.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["model", "hooks"]);
    // Orca entries untouched
    assert_eq!(installed["hooks"]["PermissionRequest"], original["hooks"]["PermissionRequest"]);
    assert_eq!(installed["hooks"]["Stop"][0], original["hooks"]["Stop"][0]);
    // Nyx entries use the absolute path of the exe
    let cmd = installed["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"].as_str().unwrap();
    assert!(cmd.contains("nyx-hook") && cmd.ends_with("event UserPromptSubmit"), "{cmd}");

    // second install: idempotent, no change, no new backup
    let out = settings_cmd(dir.path(), "install", &f, &[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(backups(dir.path()).len(), 1);
    let again: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(again, installed);

    let out = settings_cmd(dir.path(), "uninstall", &f, &[]);
    assert_eq!(out.status.code(), Some(0));
    let restored: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(restored, original);
}

#[test]
fn install_creates_missing_file_and_rejects_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("nested").join("settings.json");
    assert_eq!(settings_cmd(dir.path(), "install", &f, &[]).status.code(), Some(0));
    assert!(f.exists());

    let bad = dir.path().join("bad.json");
    std::fs::write(&bad, "{ not json").unwrap();
    let out = settings_cmd(dir.path(), "install", &bad, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), "{ not json");
}
