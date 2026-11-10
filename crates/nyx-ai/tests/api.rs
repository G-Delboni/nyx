//! nyx-ai against a fake local HTTP server. No real API call is ever made.

use nyx_ai::*;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

struct Req {
    head: String,
    body: Value,
}

struct Reply {
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    body: String,
}

fn ok(body: Value) -> Reply {
    Reply { status: 200, headers: vec![], body: body.to_string() }
}

fn err(status: u16, kind: &str, headers: Vec<(&'static str, &'static str)>) -> Reply {
    Reply {
        status,
        headers,
        body: json!({"type":"error","error":{"type":kind,"message":"falhou"}}).to_string(),
    }
}

/// Serves the scripted replies, one connection each. Returns the base URL and the
/// received requests.
fn serve(replies: Vec<Reply>) -> (String, mpsc::Receiver<Req>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for reply in replies {
            let Ok((mut s, _)) = listener.accept() else { return };
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            let (head, body) = loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break (String::new(), Vec::new());
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if buf.len() >= pos + 4 + len {
                        break (head, buf[pos + 4..pos + 4 + len].to_vec());
                    }
                }
            };
            let mut out = format!(
                "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                reply.status,
                reply.body.len()
            );
            for (k, v) in &reply.headers {
                out.push_str(&format!("{k}: {v}\r\n"));
            }
            out.push_str("\r\n");
            out.push_str(&reply.body);
            let _ = s.write_all(out.as_bytes());
            let _ = tx.send(Req { head, body: serde_json::from_slice(&body).unwrap_or(Value::Null) });
        }
    });
    (base, rx)
}

fn client(base: &str) -> Client {
    let mut c = Client::new("sk-test-123", base);
    c.backoff = Duration::from_millis(10);
    c
}

fn text_reply(text: &str, input: u64, output: u64) -> Reply {
    ok(json!({"id":"msg_1","type":"message","role":"assistant","model":"claude-haiku-5-5",
              "content":[{"type":"text","text":text}],"stop_reason":"end_turn",
              "usage":{"input_tokens":input,"output_tokens":output}}))
}

#[test]
fn chat_request_shape_and_text_reply() {
    let (base, rx) = serve(vec![text_reply("Dois agentes rodando, Dev.", 1000, 100)]);
    let history: Vec<ChatTurn> = (0..30)
        .map(|i| ChatTurn {
            role: if i % 2 == 0 { Role::User } else { Role::Assistant },
            text: format!("msg {i}"),
        })
        .collect();
    let r = client(&base).chat("agentes: 2 rodando", &history, "Como estamos?").unwrap();
    assert_eq!(r.text, "Dois agentes rodando, Dev.");
    assert!(r.actions.is_empty());
    assert_eq!(r.usage.input_tokens, 1000);
    // 1000 * 0.10 + 100 * 0.50 per million
    assert!((r.cost_usd - 0.00015).abs() < 1e-12, "{}", r.cost_usd);

    let req = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let head = req.head.to_ascii_lowercase();
    assert!(req.head.starts_with("POST /v1/messages HTTP/1.1"), "{}", req.head);
    assert!(head.contains("x-api-key: sk-test-123"));
    assert!(head.contains("anthropic-version: 2023-06-01"));
    assert!(head.contains("content-type: application/json"));
    let b = &req.body;
    assert_eq!(b["model"], DEFAULT_MODEL);
    let system = b["system"].as_str().unwrap();
    assert!(system.contains("Nyx") && system.contains("agentes: 2 rodando"));
    assert!(system.contains("Sem exclamação"));
    assert_eq!(b["tools"].as_array().unwrap().len(), 6);
    for k in ["temperature", "top_p", "top_k"] {
        assert!(b.get(k).is_none(), "{k} must not be sent");
    }
    // 20 turns of history (30 given) + the new user text, merged into the last user turn
    let msgs = b["messages"].as_array().unwrap();
    assert_eq!(msgs.first().unwrap()["role"], "user");
    assert_eq!(msgs.last().unwrap()["role"], "user");
    assert!(msgs.last().unwrap()["content"].as_str().unwrap().ends_with("Como estamos?"));
    assert!(msgs.len() <= 21);
    assert!(!msgs.iter().any(|m| m["content"].as_str().unwrap().contains("msg 5\n") || m["content"] == "msg 0"));
    for w in msgs.windows(2) {
        assert_ne!(w[0]["role"], w[1]["role"], "roles must alternate");
    }
}

#[test]
fn tool_use_becomes_proposed_actions() {
    let (base, _rx) = serve(vec![ok(json!({
        "content":[
            {"type":"thinking","thinking":""},
            {"type":"text","text":"Posso iniciar a delegação."},
            {"type":"tool_use","id":"toolu_1","name":"propose_start_delegation",
             "input":{"label":"Iniciar","title":"Corrigir testes","steps":["a","b"],"contextId":"c1"}},
            {"type":"tool_use","id":"toolu_2","name":"open_panel","input":{"tab":"delegate"}},
            {"type":"tool_use","id":"toolu_3","name":"rm_rf","input":{}}
        ],
        "stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":5}}))]);
    let r = client(&base).chat("", &[], "Delega isso").unwrap();
    assert_eq!(r.text, "Posso iniciar a delegação.");
    assert_eq!(r.actions.len(), 2, "unknown tools are dropped");
    let a = &r.actions[0];
    assert_eq!((a.id.as_str(), a.kind.as_str(), a.label.as_str()), ("toolu_1", "startDelegation", "Iniciar"));
    assert_eq!(a.args["title"], "Corrigir testes");
    assert_eq!(a.args["steps"], json!(["a", "b"]));
    assert!(a.args.get("label").is_none());
    assert_eq!((r.actions[1].kind.as_str(), r.actions[1].label.as_str()), ("openPanel", "Abrir"));
}

#[test]
fn tool_only_reply_gets_a_default_text() {
    let (base, _rx) = serve(vec![ok(json!({
        "content":[{"type":"tool_use","id":"t","name":"propose_install_hooks","input":{"label":"Instalar"}}],
        "usage":{"input_tokens":1,"output_tokens":1}}))]);
    let r = client(&base).chat("", &[], "instala os hooks").unwrap();
    assert!(r.text.contains("Confirme"));
    assert_eq!(r.actions[0].kind, "installHooks");
}

#[test]
fn retries_529_and_429_then_succeeds() {
    let (base, rx) = serve(vec![
        err(529, "overloaded_error", vec![]),
        err(429, "rate_limit_error", vec![("retry-after", "0")]),
        text_reply("ok", 1, 1),
    ]);
    let r = client(&base).chat("", &[], "oi").unwrap();
    assert_eq!(r.text, "ok");
    for _ in 0..3 {
        rx.recv_timeout(Duration::from_secs(2)).expect("request recorded");
    }
}

#[test]
fn gives_up_after_three_attempts() {
    let (base, rx) = serve(vec![err(500, "api_error", vec![]), err(503, "api_error", vec![]), err(500, "api_error", vec![]), text_reply("nunca", 1, 1)]);
    let e = client(&base).chat("", &[], "oi").unwrap_err();
    assert!(matches!(e, AiError::Api { status: 500, .. }), "{e:?}");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rx.try_iter().count(), 3, "exactly 3 attempts");
}

#[test]
fn client_errors_are_not_retried() {
    let (base, rx) = serve(vec![err(401, "authentication_error", vec![]), text_reply("nunca", 1, 1)]);
    let e = client(&base).chat("", &[], "oi").unwrap_err();
    match e {
        AiError::Api { status, kind, .. } => assert_eq!((status, kind.as_str()), (401, "authentication_error")),
        other => panic!("{other:?}"),
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rx.try_iter().count(), 1);
}

#[test]
fn timeout_is_an_error_not_a_hang() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let _hold = std::thread::spawn(move || {
        let _c: Vec<_> = listener.incoming().take(2).collect();
        std::thread::sleep(Duration::from_secs(3));
    });
    let mut c = client(&base);
    c.timeout = Duration::from_millis(300);
    c.max_attempts = 2;
    let t = std::time::Instant::now();
    let e = c.chat("", &[], "oi").unwrap_err();
    assert!(matches!(e, AiError::Transport(_)), "{e:?}");
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[test]
fn summarize_step_limits_to_five_lines() {
    let (base, rx) = serve(vec![text_reply("l1\n\nl2\nl3\nl4\nl5\nl6\nl7", 200, 50)]);
    let g = client(&base).summarize_step("Etapa 1 concluída", " 2 files changed").unwrap();
    assert_eq!(g.text.lines().count(), 5);
    assert!(g.cost_usd > 0.0);
    let req = rx.recv().unwrap();
    assert!(req.body["messages"][0]["content"].as_str().unwrap().contains("2 files changed"));
    assert!(req.body.get("tools").is_none());
}

#[test]
fn plan_steps_parses_and_clamps() {
    let list = "1. Ler o código\n2) Escrever teste\n- Corrigir\n* Rodar testes\n5. a\n6. b\n7. c\n8. d\n9. e\n10. f";
    let (base, _rx) = serve(vec![text_reply(list, 100, 60)]);
    let p = client(&base).plan_steps("Corrigir login", "projeto X").unwrap();
    assert_eq!(p.steps.len(), 8);
    assert_eq!(&p.steps[..4], ["Ler o código", "Escrever teste", "Corrigir", "Rodar testes"]);
}

#[test]
fn empty_answers_are_errors() {
    let (base, _rx) = serve(vec![text_reply("  ", 1, 1), text_reply("\n", 1, 1)]);
    let c = client(&base);
    assert!(matches!(c.summarize_step("r", ""), Err(AiError::BadResponse(_))));
    assert!(matches!(c.plan_steps("g", ""), Err(AiError::BadResponse(_))));
}

#[test]
fn cost_table() {
    let u = Usage { input_tokens: 1_000_000, output_tokens: 1_000_000, cache_creation_input_tokens: 1_000_000, cache_read_input_tokens: 1_000_000 };
    assert!((cost_usd(&u, "claude-haiku-5-5") - (0.10 + 0.50 + 0.125 + 0.01)).abs() < 1e-9);
    assert!((cost_usd(&u, "claude-haiku-4-5") - (1.0 + 5.0 + 1.25 + 0.10)).abs() < 1e-9);
    assert!(cost_usd(&u, "who-knows") > cost_usd(&u, "claude-haiku-5-5"));
}

#[test]
fn debug_never_prints_the_key() {
    let c = Client::new("sk-secret-xyz", "http://x");
    assert!(!format!("{c:?}").contains("sk-secret-xyz"));
}

#[test]
fn env_handling() {
    // single test touching the environment, to avoid races
    std::env::remove_var("NYX_ANTHROPIC_API_KEY");
    std::env::remove_var("ANTHROPIC_API_KEY");
    assert!(!key_present());
    assert!(matches!(Client::from_env(), Err(AiError::NoKey)));
    std::env::set_var("ANTHROPIC_API_KEY", "  ");
    assert!(!key_present());
    std::env::set_var("ANTHROPIC_API_KEY", "sk-env");
    std::env::set_var("NYX_AI_BASE_URL", "http://127.0.0.1:9/");
    assert!(key_present());
    let c = Client::from_env().unwrap();
    assert_eq!(c.base_url, "http://127.0.0.1:9");
    assert_eq!(c.model, DEFAULT_MODEL);
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("NYX_AI_BASE_URL");
}
