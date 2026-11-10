use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

enum Resp {
    Ok(Vec<u8>),
    Status(u16),
    Hang,
}

/// Fake Azure: serves scripted responses in order and records each raw request.
fn serve(script: Vec<Resp>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let reqs = Arc::new(Mutex::new(vec![]));
    let r2 = reqs.clone();
    std::thread::spawn(move || {
        for resp in script {
            let Ok((mut s, _)) = listener.accept() else { return };
            let mut buf = vec![];
            let mut tmp = [0u8; 1024];
            let (head_end, clen) = loop {
                let n = s.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break (buf.len(), 0);
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                    let cl = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    break (i + 4, cl);
                }
            };
            while buf.len() < head_end + clen {
                let n = s.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            r2.lock().unwrap().push(String::from_utf8_lossy(&buf).to_string());
            match resp {
                Resp::Ok(body) => {
                    let head =
                        format!("HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(&body);
                }
                Resp::Status(c) => {
                    let _ = s.write_all(format!("HTTP/1.1 {c} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
                }
                Resp::Hang => std::thread::sleep(Duration::from_millis(1500)),
            }
        }
    });
    (base, reqs)
}

struct FakeWin {
    calls: Mutex<u32>,
}
impl FakeWin {
    fn new() -> Self {
        Self { calls: Mutex::new(0) }
    }
}
impl WinBackend for FakeWin {
    fn voice(&self) -> String {
        "Fake Maria".into()
    }
    fn synth(&self, _t: &str, _p: &VoiceParams) -> anyhow::Result<Vec<u8>> {
        *self.calls.lock().unwrap() += 1;
        Ok(b"RIFFfakewav".to_vec())
    }
}

fn creds() -> AzureCreds {
    AzureCreds { key: "secret-key".into(), region: "brazilsouth".into() }
}

fn synth_for(base: &str, dir: &Path) -> Synth {
    let mut s = Synth::new(dir);
    s.azure_base_url = Some(base.to_string());
    s
}

const T: Duration = Duration::from_millis(400);

#[test]
fn ssml_shape_and_escape() {
    let p = VoiceParams::default();
    let s = build_ssml("a < b & \"c\"", &p);
    assert_eq!(
        s,
        "<speak version=\"1.0\" xml:lang=\"pt-BR\"><voice name=\"pt-BR-FranciscaNeural\"><prosody rate=\"-5%\" pitch=\"-5%\">a &lt; b &amp; &quot;c&quot;</prosody></voice></speak>"
    );
}

#[test]
fn azure_success_request_shape_cache_and_counter() {
    let dir = tempfile::tempdir().unwrap();
    let (base, reqs) = serve(vec![Resp::Ok(b"MP3DATA".to_vec())]);
    let s = synth_for(&base, dir.path());
    let w = FakeWin::new();
    let a = s.synthesize_inner("Dev terminou.", &VoiceParams::default(), EnginePref::Auto, Some(&creds()), T, &w).unwrap();
    assert_eq!(a.engine, Engine::Azure);
    assert_eq!(a.format, AudioFormat::Mp3);
    assert!(!a.cached);
    assert_eq!(a.bytes, b"MP3DATA");
    let raw = reqs.lock().unwrap()[0].clone();
    let lower = raw.to_lowercase();
    assert!(raw.starts_with("POST /cognitiveservices/v1"));
    assert!(lower.contains("ocp-apim-subscription-key: secret-key"));
    assert!(lower.contains("content-type: application/ssml+xml"));
    assert!(lower.contains("x-microsoft-outputformat: audio-24khz-48kbitrate-mono-mp3"));
    assert!(lower.contains("user-agent: nyx"));
    assert!(raw.contains("Dév terminou."));
    let chars = "Dév terminou.".chars().count() as u64;
    assert_eq!(s.month_chars(), chars);
    // second call: cache hit, server has no more scripted responses, counter unchanged
    let b = s.synthesize_inner("Dev terminou.", &VoiceParams::default(), EnginePref::Auto, Some(&creds()), T, &w).unwrap();
    assert!(b.cached);
    assert_eq!(b.bytes, a.bytes);
    assert_eq!(s.month_chars(), chars);
    assert_eq!(reqs.lock().unwrap().len(), 1);
    // different text is a miss
    assert!(!dir.path().join("usage.json").to_string_lossy().is_empty());
}

#[test]
fn auto_falls_back_on_401_429_and_timeout() {
    for resp in [Resp::Status(401), Resp::Status(429), Resp::Hang] {
        let dir = tempfile::tempdir().unwrap();
        let (base, _r) = serve(vec![resp]);
        let s = synth_for(&base, dir.path());
        let w = FakeWin::new();
        let a = s.synthesize_inner("Olá", &VoiceParams::default(), EnginePref::Auto, Some(&creds()), T, &w).unwrap();
        assert_eq!(a.engine, Engine::Windows);
        assert_eq!(a.format, AudioFormat::Wav);
        assert_eq!(s.month_chars(), 0, "failed Azure calls cost nothing");
        // Windows result is cached too
        let b = s.synthesize_inner("Olá", &VoiceParams::default(), EnginePref::Windows, None, T, &w).unwrap();
        assert!(b.cached);
        assert_eq!(*w.calls.lock().unwrap(), 1);
    }
}

#[test]
fn azure_only_errors_and_windows_only_skips_azure() {
    let dir = tempfile::tempdir().unwrap();
    let (base, reqs) = serve(vec![Resp::Status(401)]);
    let s = synth_for(&base, dir.path());
    let w = FakeWin::new();
    let e = s.synthesize_inner("Olá", &VoiceParams::default(), EnginePref::Azure, Some(&creds()), T, &w).unwrap_err();
    assert!(e.to_string().contains("401"));
    assert!(!e.to_string().contains("secret-key"));
    assert!(s.synthesize_inner("Olá", &VoiceParams::default(), EnginePref::Azure, None, T, &w).is_err());
    let a = s.synthesize_inner("Olá", &VoiceParams::default(), EnginePref::Windows, Some(&creds()), T, &w).unwrap();
    assert_eq!(a.engine, Engine::Windows);
    assert_eq!(reqs.lock().unwrap().len(), 1);
    assert_eq!(*w.calls.lock().unwrap(), 1);
}

#[test]
fn auto_without_creds_uses_windows() {
    let dir = tempfile::tempdir().unwrap();
    let s = Synth::new(dir.path());
    let w = FakeWin::new();
    let a = s.synthesize_inner("Olá", &VoiceParams::default(), EnginePref::Auto, None, T, &w).unwrap();
    assert_eq!(a.engine, Engine::Windows);
}

#[test]
fn cache_key_depends_on_everything() {
    let p = VoiceParams::default();
    let k = cache::cache_key(Engine::Azure, "v", &p, "texto");
    assert_ne!(k, cache::cache_key(Engine::Windows, "v", &p, "texto"));
    assert_ne!(k, cache::cache_key(Engine::Azure, "w", &p, "texto"));
    assert_ne!(k, cache::cache_key(Engine::Azure, "v", &p, "outro"));
    let p2 = VoiceParams { rate_pct: 0, ..p.clone() };
    assert_ne!(k, cache::cache_key(Engine::Azure, "v", &p2, "texto"));
    assert_eq!(k, cache::cache_key(Engine::Azure, "v", &p, "texto"));
}

#[test]
fn lru_evicts_oldest_by_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let now = std::time::SystemTime::now();
    for (name, age) in [("a", 300u64), ("b", 200), ("c", 100)] {
        let p = d.join(format!("{name}.mp3"));
        std::fs::write(&p, vec![0u8; 100]).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_modified(now - Duration::from_secs(age)).unwrap();
    }
    // reading "a" refreshes it, so "b" becomes the oldest
    assert!(cache::get(d, "a", AudioFormat::Mp3).is_some());
    let newest = d.join("new.mp3");
    std::fs::write(&newest, vec![0u8; 100]).unwrap();
    cache::evict(d, 250, &newest);
    assert!(d.join("a.mp3").exists());
    assert!(!d.join("b.mp3").exists());
    assert!(!d.join("c.mp3").exists());
    assert!(newest.exists());
}

#[test]
fn monthly_counter_accumulates_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let s = Synth::new(dir.path());
    assert_eq!(s.month_chars(), 0);
    cache::add_chars(dir.path(), 10);
    cache::add_chars(dir.path(), 5);
    assert_eq!(Synth::new(dir.path()).month_chars(), 15);
    // an old month in the file does not count toward the current one
    let cur = chrono::Local::now().format("%Y-%m").to_string();
    std::fs::write(dir.path().join("usage.json"), format!("{{\"1999-01\":999,\"{cur}\":7}}")).unwrap();
    assert_eq!(s.month_chars(), 7);
}

#[test]
fn empty_text_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let s = Synth::new(dir.path());
    assert!(s.synthesize_inner("  ✅ ", &VoiceParams::default(), EnginePref::Windows, None, T, &FakeWin::new()).is_err());
}

/// Real Windows voice; run by hand: `cargo test -p nyx-voice -- --ignored`.
#[test]
#[ignore]
#[cfg(windows)]
fn real_windows_voice() {
    let name = windows_voice_name().expect("pt-BR female voice installed");
    println!("voice: {name}");
    let dir = tempfile::tempdir().unwrap();
    let a = Synth::new(dir.path())
        .synthesize("Dev terminou a etapa.", &VoiceParams::default(), EnginePref::Windows, None)
        .unwrap();
    assert_eq!(a.format, AudioFormat::Wav);
    assert_eq!(&a.bytes[..4], b"RIFF");
    assert!(a.bytes.len() > 10_000);
}
