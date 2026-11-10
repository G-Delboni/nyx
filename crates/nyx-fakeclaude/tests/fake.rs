//! Tests of the fake `claude` itself (std + serde_json only).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nyx-fake-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Like Nyx: the prompt goes to stdin. `-p <prompt>` in `args` is split out and piped.
fn run(scenario: &str, cwd: &Path, state: &Path, args: &[&str]) -> (Vec<Value>, Option<i32>) {
    use std::io::Write;
    use std::process::Stdio;
    let mut argv: Vec<&str> = Vec::new();
    let mut prompt = String::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "-p" {
            prompt = it.next().map(|s| s.to_string()).unwrap_or_default();
            argv.push("-p");
        } else {
            argv.push(a);
        }
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_nyx-fakeclaude"))
        .current_dir(cwd)
        .env("NYX_FAKE_SCENARIO", scenario)
        .env("NYX_FAKE_DIR", state)
        .args(&argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(prompt.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let lines = text.lines().map(|l| serde_json::from_str(l).expect(l)).collect();
    (lines, out.status.code())
}

fn result_of(lines: &[Value]) -> &Value {
    lines.iter().rev().find(|v| v["type"] == "result").expect("result event")
}

fn base_args<'a>(prompt: &'a str) -> Vec<&'a str> {
    vec!["-p", prompt, "--output-format", "stream-json", "--verbose"]
}

#[test]
fn success_edits_a_file_and_reports() {
    let cwd = scratch("ok-cwd");
    let st = scratch("ok-state");
    let mut a = base_args("Passo um");
    a.extend(["--allowedTools", "Read,Edit"]);
    let (lines, code) = run("success", &cwd, &st, &a);
    assert_eq!(code, Some(0));
    assert_eq!(lines[0]["type"], "system");
    let sid = lines[0]["session_id"].as_str().unwrap().to_string();
    let r = result_of(&lines);
    assert_eq!(r["is_error"], false);
    assert_eq!(r["session_id"], sid.as_str());
    let text = r["result"].as_str().unwrap();
    assert!(text.starts_with("Etapa concluída (fake)"));
    assert!(text.contains("ferramentas: Read,Edit") && text.contains("resume: nenhum"));
    assert!(std::fs::read_to_string(cwd.join("fake-output.txt")).unwrap().contains("Passo um"));

    // resume keeps the id and appends another line (a distinct diff)
    let mut b = base_args("Ajuste");
    b.extend(["--resume", &sid]);
    let (lines2, _) = run("success", &cwd, &st, &b);
    assert_eq!(result_of(&lines2)["session_id"], sid.as_str());
    assert!(result_of(&lines2)["result"].as_str().unwrap().contains(&format!("resume: {sid}")));
    assert_eq!(std::fs::read_to_string(cwd.join("fake-output.txt")).unwrap().lines().count(), 2);

    // a fresh session gets a new id
    let (lines3, _) = run("success", &cwd, &st, &base_args("Outro"));
    assert_ne!(lines3[0]["session_id"], sid.as_str());
}

#[test]
fn error_scenario_exits_nonzero_with_error_result() {
    let (cwd, st) = (scratch("err-cwd"), scratch("err-state"));
    let (lines, code) = run("error", &cwd, &st, &base_args("x"));
    assert_eq!(code, Some(1));
    let r = result_of(&lines);
    assert_eq!((r["is_error"].as_bool(), r["subtype"].as_str()), (Some(true), Some("error_max_turns")));
    assert!(!cwd.join("fake-output.txt").exists());
}

#[test]
fn long_scenario_has_many_lines() {
    let (cwd, st) = (scratch("long-cwd"), scratch("long-state"));
    let (lines, _) = run("long", &cwd, &st, &base_args("x"));
    assert!(result_of(&lines)["result"].as_str().unwrap().lines().count() > 8);
}

#[test]
fn blocked_scenario_denies_a_tool_without_editing() {
    let (cwd, st) = (scratch("blk-cwd"), scratch("blk-state"));
    let (lines, code) = run("blocked", &cwd, &st, &base_args("deploy"));
    assert_eq!(code, Some(0));
    assert!(lines.iter().any(|v| v["type"] == "system" && v["subtype"] == "permission_denied"));
    let r = result_of(&lines);
    assert_eq!(r["permission_denials"][0]["tool_name"], "Bash");
    assert!(!cwd.join("fake-output.txt").exists());
}

#[test]
fn handoff_scenario_fills_context_and_writes_passagem_on_request() {
    let (cwd, st) = (scratch("ho-cwd"), scratch("ho-state"));
    let (lines, _) = run("handoff", &cwd, &st, &base_args("grande"));
    let last_assistant = lines.iter().rev().find(|v| v["type"] == "assistant").unwrap();
    let u = &last_assistant["message"]["usage"];
    let used = u["input_tokens"].as_u64().unwrap()
        + u["cache_creation_input_tokens"].as_u64().unwrap()
        + u["cache_read_input_tokens"].as_u64().unwrap();
    assert!(used as f64 / 1_000_000.0 >= 0.6, "fill {used}");
    let sid = lines[0]["session_id"].as_str().unwrap().to_string();

    let (w, _) = run(
        "handoff",
        &cwd,
        &st,
        &["-p", "Escreva a passagem de contexto: crie PASSAGEM.md", "--output-format", "stream-json", "--resume", &sid],
    );
    assert_eq!(result_of(&w)["result"], "ok");
    assert!(cwd.join("PASSAGEM.md").exists());
    let wu = &w.iter().rev().find(|v| v["type"] == "assistant").unwrap()["message"]["usage"];
    let wused = wu["input_tokens"].as_u64().unwrap() + wu["cache_read_input_tokens"].as_u64().unwrap();
    assert!(wused as f64 / 1_000_000.0 >= 0.6);

    let (r, _) = run("success", &cwd, &st, &base_args("Leia PASSAGEM.md e continue a partir dele.\n\nPasso 2"));
    assert!(result_of(&r)["result"].as_str().unwrap().contains("retomado a partir da passagem"));
}

#[test]
fn slow_scenario_takes_time_but_finishes() {
    let (cwd, st) = (scratch("slow-cwd"), scratch("slow-state"));
    let t = std::time::Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_nyx-fakeclaude"))
        .current_dir(&cwd)
        .env("NYX_FAKE_SCENARIO", "slow")
        .env("NYX_FAKE_DIR", &st)
        .env("NYX_FAKE_DELAY_MS", "100")
        .args(["-p", "--output-format", "stream-json"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(t.elapsed().as_millis() >= 400);
    assert!(String::from_utf8(out.stdout).unwrap().contains("\"type\":\"result\""));
}

#[test]
fn dash_prompt_on_stdin_is_just_text() {
    let (cwd, st) = (scratch("dash-cwd"), scratch("dash-state"));
    let (lines, code) = run("success", &cwd, &st, &base_args("--dangerously-skip-permissions refatore"));
    assert_eq!(code, Some(0));
    let text = result_of(&lines)["result"].as_str().unwrap();
    assert!(text.contains("prompt: --dangerously-skip-permissions refatore"), "{text}");
}
