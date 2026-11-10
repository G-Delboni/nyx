//! Fake `claude` for tests. Spec: docs/specs/NOITE-2.md §2. Owner: agent B.
//!
//! Behaves like `claude -p --output-format stream-json --verbose [--resume ID]
//! [--allowedTools LIST] ...` with the prompt on stdin, and prints synthetic stream-json. Never talks to any API.
//!
//! Env:
//! - `NYX_FAKE_SCENARIO` = `success` (default) | `error` | `long` | `handoff` | `blocked` | `slow`
//! - `NYX_FAKE_DIR`      = state dir (call counter, fresh-session counter); default: temp dir.
//! - `NYX_FAKE_DELAY_MS` = pause between lines in `slow` (default 400).
//!
//! Scenarios that "edit" append a line to `<cwd>/fake-output.txt`, so every step and
//! every retry produces a distinct git diff. A prompt starting with the handoff-write
//! marker writes `<cwd>/PASSAGEM.md` and answers "ok" (any scenario). `handoff` ends
//! with a context fill of about 0.65 on a 1M window.

use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

const MODEL: &str = "claude-opus-5-5";
const HANDOFF_WRITE_PREFIX: &str = "Escreva a passagem de contexto";

struct Args {
    prompt: String,
    resume: Option<String>,
    allowed_tools: Option<String>,
}

fn parse_args() -> Args {
    let mut a = Args { prompt: String::new(), resume: None, allowed_tools: None };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(x) = it.next() {
        match x.as_str() {
            // `-p` is a boolean flag; the prompt is stdin (what Nyx sends) or, like the
            // real CLI, a positional argument that does not look like an option.
            "-p" | "--print" => {
                if it.peek().map_or(false, |n| !n.starts_with('-')) {
                    a.prompt = it.next().unwrap_or_default();
                }
            }
            "--resume" | "-r" => a.resume = it.next(),
            "--allowedTools" | "--allowed-tools" => a.allowed_tools = it.next(),
            // flags with a value we ignore
            "--output-format" | "--model" | "--permission-mode" | "--append-system-prompt" => {
                it.next();
            }
            _ => {}
        }
    }
    if a.prompt.is_empty() {
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut a.prompt);
    }
    a
}

fn state_dir() -> PathBuf {
    let d = std::env::var("NYX_FAKE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("nyx-fakeclaude"));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Increments and returns a counter stored in `<state>/<name>`.
fn bump(name: &str) -> u64 {
    let p = state_dir().join(name);
    let n = std::fs::read_to_string(&p).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0) + 1;
    let _ = std::fs::write(&p, n.to_string());
    n
}

struct Out {
    delay: u64,
}

impl Out {
    fn line(&self, v: Value) {
        let mut so = std::io::stdout().lock();
        let _ = writeln!(so, "{v}");
        let _ = so.flush();
        if self.delay > 0 {
            drop(so);
            std::thread::sleep(std::time::Duration::from_millis(self.delay));
        }
    }
}

fn usage(input: u64, output: u64, write: u64, read: u64) -> Value {
    json!({"input_tokens": input, "output_tokens": output,
           "cache_creation_input_tokens": write, "cache_read_input_tokens": read})
}

fn assistant(sid: &str, id: &str, content: Value, u: Value) -> Value {
    json!({"type":"assistant","session_id":sid,"parent_tool_use_id":null,
           "message":{"id":id,"model":MODEL,"role":"assistant","content":content,"usage":u}})
}

fn tool_result(sid: &str, tool_id: &str, text: &str, is_error: bool) -> Value {
    json!({"type":"user","session_id":sid,"parent_tool_use_id":null,
           "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":tool_id,
                                                "content":text,"is_error":is_error}]}})
}

fn short(s: &str, n: usize) -> String {
    let one = s.lines().next().unwrap_or("");
    one.chars().take(n).collect()
}

fn main() {
    let args = parse_args();
    let scenario = std::env::var("NYX_FAKE_SCENARIO").unwrap_or_else(|_| "success".into());
    let call = bump("calls");
    let sid = match &args.resume {
        Some(id) => id.clone(),
        None => format!("fake-sess-{}", bump("fresh")),
    };
    let delay = if scenario == "slow" {
        std::env::var("NYX_FAKE_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(400)
    } else {
        0
    };
    let out = Out { delay };
    let cwd = std::env::current_dir().unwrap_or_default();

    out.line(json!({"type":"system","subtype":"init","session_id":sid,"cwd":cwd.to_string_lossy(),
                    "model":MODEL,"tools":["Bash","Edit","Read"]}));

    // Handoff-write step: write PASSAGEM.md, answer "ok".
    if args.prompt.starts_with(HANDOFF_WRITE_PREFIX) {
        let body = format!("# Passagem\n\n- objetivo: teste (fake)\n- chamada {call}\n");
        let _ = std::fs::write(cwd.join("PASSAGEM.md"), body);
        out.line(assistant(
            &sid,
            &format!("msg_{call}_1"),
            json!([{"type":"tool_use","id":"toolu_h","name":"Write","input":{"file_path":"PASSAGEM.md"}}]),
            usage(3, 80, 0, 640_000),
        ));
        out.line(tool_result(&sid, "toolu_h", "ok", false));
        out.line(assistant(&sid, &format!("msg_{call}_2"), json!([{"type":"text","text":"ok"}]), usage(3, 5, 0, 640_000)));
        finish(&out, &sid, "success", false, "ok", 2, usage(6, 85, 0, 1_280_000));
        return;
    }

    let resumed_from_handoff = args.prompt.starts_with("Leia PASSAGEM.md");
    let prompt_hint = short(args.prompt.trim_start_matches("Leia PASSAGEM.md e continue a partir dele.").trim(), 60);
    let tools = args.allowed_tools.clone().unwrap_or_else(|| "(nenhuma)".into());

    match scenario.as_str() {
        "blocked" => {
            let input = json!({"command":"npm run deploy","description":"Deploy"});
            out.line(assistant(
                &sid,
                &format!("msg_{call}_1"),
                json!([{"type":"text","text":"Preciso rodar o deploy."},
                       {"type":"tool_use","id":"toolu_b","name":"Bash","input":input}]),
                usage(3, 60, 2000, 0),
            ));
            out.line(tool_result(
                &sid,
                "toolu_b",
                "Claude requested permissions to use Bash, but you haven't granted it yet.",
                true,
            ));
            out.line(json!({"type":"system","subtype":"permission_denied","session_id":sid,
                            "tool_name":"Bash","tool_use_id":"toolu_b","tool_input":input}));
            out.line(assistant(
                &sid,
                &format!("msg_{call}_2"),
                json!([{"type":"text","text":"Não consegui rodar o deploy: falta permissão para Bash."}]),
                usage(3, 40, 0, 2000),
            ));
            let result = "Bloqueado pela falta de permissão\n\nBash(npm run deploy) foi negado.\nNada foi alterado.";
            let mut v = result_event(&sid, "success", false, result, 2, usage(6, 100, 2000, 2000));
            v["permission_denials"] = json!([{"tool_name":"Bash","tool_use_id":"toolu_b","tool_input":input}]);
            out.line(v);
        }
        "error" => {
            out.line(assistant(
                &sid,
                &format!("msg_{call}_1"),
                json!([{"type":"tool_use","id":"toolu_e","name":"Bash","input":{"command":"npm run build"}}]),
                usage(3, 90, 2000, 0),
            ));
            out.line(tool_result(&sid, "toolu_e", "build failed", true));
            let mut v = result_event(&sid, "error_max_turns", true, "", 30, usage(90, 4000, 2000, 300_000));
            v.as_object_mut().unwrap().remove("result");
            out.line(v);
            std::process::exit(1);
        }
        _ => {
            // success | long | handoff | slow: "edit" a file in the cwd, then report.
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(cwd.join("fake-output.txt"))
                .expect("fake-output.txt");
            let _ = writeln!(f, "call {call}: {prompt_hint}");
            out.line(assistant(
                &sid,
                &format!("msg_{call}_1"),
                json!([{"type":"text","text":"Vou editar o arquivo."},
                       {"type":"tool_use","id":"toolu_1","name":"Edit","input":{"file_path":"fake-output.txt"}}]),
                usage(3, 100, 4000, 0),
            ));
            out.line(tool_result(&sid, "toolu_1", "ok", false));
            let (cache_read, fill_note) = if scenario == "handoff" { (650_000, true) } else { (12_000, false) };
            out.line(assistant(
                &sid,
                &format!("msg_{call}_2"),
                json!([{"type":"text","text":"Etapa concluída."}]),
                usage(3, 150, 0, cache_read),
            ));
            let mut text = String::from("Etapa concluída (fake)\n\n");
            text.push_str(&format!("prompt: {prompt_hint}\n"));
            text.push_str(&format!("resume: {}\n", args.resume.as_deref().unwrap_or("nenhum")));
            text.push_str(&format!("ferramentas: {tools}"));
            if resumed_from_handoff {
                text.push_str("\nretomado a partir da passagem");
            }
            if fill_note {
                text.push_str("\ncontexto alto");
            }
            if scenario == "long" {
                for i in 1..=8 {
                    text.push_str(&format!("\nlinha extra {i}"));
                }
            }
            out.line(result_event(&sid, "success", false, &text, 3, usage(6, 250, 4000, cache_read * 2)));
        }
    }
}

fn result_event(sid: &str, subtype: &str, is_error: bool, text: &str, turns: u64, u: Value) -> Value {
    json!({"type":"result","subtype":subtype,"is_error":is_error,"duration_ms":1234,"duration_api_ms":1100,
           "num_turns":turns,"result":text,"session_id":sid,"total_cost_usd":0.0123,"usage":u})
}

fn finish(out: &Out, sid: &str, subtype: &str, is_error: bool, text: &str, turns: u64, u: Value) {
    out.line(result_event(sid, subtype, is_error, text, turns, u));
}
