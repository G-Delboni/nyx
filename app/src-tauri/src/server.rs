//! Servidor de eventos local: `POST /v1/events` em 127.0.0.1 (porta do SO),
//! autenticado por `X-Nyx-Token`. Responde 202 e entrega o envelope a um canal;
//! o processamento acontece fora do request.

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use nyx_claude::{HookEnvelope, RuntimeInfo};
use std::path::Path;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;

pub const MAX_BODY: usize = 256 * 1024;
pub const TOKEN_HEADER: &str = "x-nyx-token";

#[derive(Clone)]
struct ServerState {
    token: Arc<String>,
    tx: SyncSender<HookEnvelope>,
}

/// Token aleatório de 32 caracteres hex (16 bytes).
pub fn new_token() -> String {
    let b: [u8; 16] = rand::random();
    hex::encode(b)
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn post_event(State(st): State<ServerState>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let ok = headers
        .get(TOKEN_HEADER)
        .map(|v| ct_eq(v.as_bytes(), st.token.as_bytes()))
        .unwrap_or(false);
    if !ok {
        return StatusCode::UNAUTHORIZED;
    }
    let env: HookEnvelope = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(_) => return StatusCode::BAD_REQUEST,
    };
    match st.tx.try_send(env) {
        Ok(()) => StatusCode::ACCEPTED,
        Err(TrySendError::Full(_)) => StatusCode::SERVICE_UNAVAILABLE,
        Err(TrySendError::Disconnected(_)) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

pub fn router(token: String, tx: SyncSender<HookEnvelope>) -> Router {
    Router::new()
        .route("/v1/events", post(post_event))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(ServerState { token: Arc::new(token), tx })
}

/// Liga o socket já aqui (síncrono) para saber a porta antes de escrever o runtime.json.
pub fn bind() -> std::io::Result<std::net::TcpListener> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    l.set_nonblocking(true)?;
    Ok(l)
}

pub async fn serve(listener: std::net::TcpListener, app: Router) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::from_std(listener)?;
    axum::serve(listener, app).await
}

/// Escreve o runtime.json de forma atômica (tmp + rename).
pub fn write_runtime(path: &Path, info: &RuntimeInfo) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(info)?)?;
    std::fs::rename(&tmp, path)
}

/// Apaga o runtime.json só se ainda for o desta instância (pid igual).
pub fn remove_runtime(path: &Path) {
    let mine = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<RuntimeInfo>(&b).ok())
        .map(|i| i.pid == std::process::id())
        .unwrap_or(false);
    if mine {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn send(port: u16, token: Option<&str>, body: &[u8]) -> u16 {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut req = format!(
            "POST /v1/events HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        if let Some(t) = token {
            req.push_str(&format!("X-Nyx-Token: {t}\r\n"));
        }
        req.push_str("\r\n");
        s.write_all(req.as_bytes()).await.unwrap();
        let _ = s.write_all(body).await;
        let mut buf = vec![];
        let _ = s.read_to_end(&mut buf).await;
        let text = String::from_utf8_lossy(&buf);
        text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
    }

    #[tokio::test]
    async fn auth_size_and_delivery() {
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let token = new_token();
        assert_eq!(token.len(), 32);
        let l = bind().unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(serve(l, router(token.clone(), tx)));

        let body = br#"{"source":"claude-code","hook":"Stop","received_at":"2026-11-07T03:00:00Z","payload":{"session_id":"s1"}}"#;
        assert_eq!(send(port, None, body).await, 401);
        assert_eq!(send(port, Some("wrong"), body).await, 401);
        assert_eq!(send(port, Some(&token), b"{not json").await, 400);
        assert_eq!(send(port, Some(&token), body).await, 202);
        let env = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(env.hook, "Stop");
        assert_eq!(env.payload["session_id"], "s1");

        let big = vec![b' '; MAX_BODY + 10];
        assert_eq!(send(port, Some(&token), &big).await, 413);
    }

    #[test]
    fn runtime_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Nyx").join("runtime.json");
        write_runtime(&p, &RuntimeInfo { port: 1234, token: "ab".into(), pid: std::process::id() }).unwrap();
        let back: RuntimeInfo = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(back.port, 1234);
        remove_runtime(&p);
        assert!(!p.exists());
        // runtime de outro processo não é apagado
        write_runtime(&p, &RuntimeInfo { port: 1, token: "x".into(), pid: 1 }).unwrap();
        remove_runtime(&p);
        assert!(p.exists());
    }
}
