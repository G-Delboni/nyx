//! Disk cache (LRU by mtime) and the monthly Azure character counter.
use crate::{AudioFormat, Engine, VoiceParams};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn cache_key(engine: Engine, voice: &str, p: &VoiceParams, text: &str) -> String {
    let mut h = Sha256::new();
    for part in [
        format!("{:?}", engine),
        voice.to_string(),
        p.rate_pct.to_string(),
        p.pitch_pct.to_string(),
        text.to_string(),
    ] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    hex::encode(h.finalize())
}

fn ext(f: AudioFormat) -> &'static str {
    match f {
        AudioFormat::Mp3 => "mp3",
        AudioFormat::Wav => "wav",
    }
}

fn path(dir: &Path, key: &str, f: AudioFormat) -> PathBuf {
    dir.join(format!("{}.{}", key, ext(f)))
}

pub fn get(dir: &Path, key: &str, f: AudioFormat) -> Option<Vec<u8>> {
    let p = path(dir, key, f);
    let bytes = fs::read(&p).ok()?;
    if bytes.is_empty() {
        return None;
    }
    if let Ok(file) = fs::OpenOptions::new().write(true).open(&p) {
        let _ = file.set_modified(SystemTime::now());
    }
    Some(bytes)
}

pub fn put(dir: &Path, key: &str, f: AudioFormat, bytes: &[u8], max_bytes: u64) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let p = path(dir, key, f);
    let _ = fs::write(&p, bytes);
    evict(dir, max_bytes, &p);
}

pub fn evict(dir: &Path, max_bytes: u64, keep: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = rd
        .flatten()
        .filter(|e| matches!(e.path().extension().and_then(|x| x.to_str()), Some("mp3") | Some("wav")))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, len, p) in files {
        if total <= max_bytes {
            break;
        }
        if p == keep {
            continue;
        }
        if fs::remove_file(&p).is_ok() {
            total -= len;
        }
    }
}

fn month_key() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

fn usage_path(dir: &Path) -> PathBuf {
    dir.join("usage.json")
}

fn read_usage(dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    fs::read_to_string(usage_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

pub fn month_chars(dir: &Path) -> u64 {
    read_usage(dir).get(&month_key()).and_then(|v| v.as_u64()).unwrap_or(0)
}

pub fn add_chars(dir: &Path, n: u64) {
    let _ = fs::create_dir_all(dir);
    let mut m = read_usage(dir);
    let cur = m.get(&month_key()).and_then(|v| v.as_u64()).unwrap_or(0);
    m.insert(month_key(), serde_json::json!(cur + n));
    let _ = fs::write(usage_path(dir), serde_json::Value::Object(m).to_string());
}
