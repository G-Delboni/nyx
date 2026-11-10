//! Watcher dos transcripts (`~/.claude/projects`, SÓ LEITURA): `notify` com debounce
//! de 2 s, catch-up pelos offsets salvos no início, lê só os bytes novos.

use crate::core;
use crate::db::{Db, FileOffset};
use anyhow::Result;
use chrono::Utc;
use nyx_claude::UsageRecord;
use notify::{EventKind as NEK, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

pub const DEBOUNCE: Duration = Duration::from_secs(2);
/// Em escrita contínua, processa pelo menos a cada 10 s.
pub const MAX_WAIT: Duration = Duration::from_secs(10);

pub type Reader = fn(&Path, u64) -> std::io::Result<(Vec<UsageRecord>, u64)>;
pub type Coster = fn(&UsageRecord) -> f64;

fn is_transcript(p: &Path) -> bool {
    p.extension().map(|e| e.eq_ignore_ascii_case("jsonl")).unwrap_or(false)
}

fn file_stamp(p: &Path) -> Option<(u64, i64)> {
    let m = std::fs::metadata(p).ok()?;
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some((m.len(), mtime))
}

/// Lê os bytes novos de um transcript e grava o uso. Devolve quantas linhas mudaram.
pub fn process_file(db: &Mutex<Db>, path: &Path, read: Reader, cost: Coster) -> Result<usize> {
    let Some((size, mtime)) = file_stamp(path) else { return Ok(0) };
    let key = path.to_string_lossy().to_string();
    let saved = db.lock().unwrap().get_offset(&key)?;
    if let Some(s) = saved {
        if s.size == size && s.mtime == mtime && s.offset <= size {
            return Ok(0); // nada novo
        }
    }
    let from = saved.map(|s| s.offset).unwrap_or(0);
    // Leitura fora do lock do banco.
    let (records, new_offset) = read(path, from)?;
    let mut db = db.lock().unwrap();
    let n = core::ingest_usage(&mut db, records, cost, Utc::now())?;
    db.set_offset(&key, FileOffset { offset: new_offset, mtime, size })?;
    Ok(n)
}

/// Todos os `.jsonl` sob `root` (recursivo; inclui transcripts de subagentes).
pub fn list_transcripts(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() && is_transcript(&p) => out.push(p),
                _ => {}
            }
        }
    }
    out
}

pub fn catch_up(db: &Mutex<Db>, root: &Path, read: Reader, cost: Coster) -> usize {
    let mut total = 0;
    for p in list_transcripts(root) {
        match process_file(db, &p, read, cost) {
            Ok(n) => total += n,
            Err(e) => log::warn!("transcript {}: {e:#}", p.display()),
        }
    }
    total
}

fn collect(ev: notify::Result<notify::Event>, into: &mut HashSet<PathBuf>) {
    let Ok(ev) = ev else { return };
    if matches!(ev.kind, NEK::Create(_) | NEK::Modify(_) | NEK::Any) {
        into.extend(ev.paths.into_iter().filter(|p| is_transcript(p)));
    }
}

/// Sobe a thread do watcher: catch-up e depois só eventos do SO (sem polling).
/// `on_change` roda quando algum uso novo foi gravado.
pub fn spawn(
    db: Arc<Mutex<Db>>,
    root: PathBuf,
    read: Reader,
    cost: Coster,
    on_change: impl Fn() + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name("nyx-transcripts".into()).spawn(move || {
        // Sem a pasta ainda (Claude Code nunca rodou): confere a cada 5 min, sem criar nada lá.
        while !root.is_dir() {
            std::thread::sleep(Duration::from_secs(300));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                log::error!("watcher: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
            log::error!("watcher em {}: {e}", root.display());
            return;
        }
        // Catch-up depois de armar o watcher: nada escrito no meio se perde.
        let t0 = Instant::now();
        let n = catch_up(&db, &root, read, cost);
        log::info!("catch-up dos transcripts: {n} registros em {} ms", t0.elapsed().as_millis());
        if n > 0 {
            on_change();
        }
        let mut pending = HashSet::new();
        while let Ok(ev) = rx.recv() {
            collect(ev, &mut pending);
            let started = Instant::now();
            loop {
                match rx.recv_timeout(DEBOUNCE) {
                    Ok(ev) => {
                        collect(ev, &mut pending);
                        if started.elapsed() >= MAX_WAIT {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            let mut changed = 0;
            for p in pending.drain() {
                match process_file(&db, &p, read, cost) {
                    Ok(n) => changed += n,
                    Err(e) => log::warn!("transcript {}: {e:#}", p.display()),
                }
            }
            if changed > 0 {
                on_change();
            }
        }
        drop(watcher);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::io::Write;

    /// Leitor falso: cada linha completa "id,input" vira um registro.
    fn fake_read(path: &Path, offset: u64) -> std::io::Result<(Vec<UsageRecord>, u64)> {
        let data = std::fs::read(path)?;
        let start = (offset as usize).min(data.len());
        let tail = &data[start..];
        let complete = tail.iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
        let recs = String::from_utf8_lossy(&tail[..complete])
            .lines()
            .filter_map(|l| {
                let (id, n) = l.split_once(',')?;
                Some(UsageRecord {
                    message_id: id.into(),
                    session_id: "s".into(),
                    cwd: None,
                    model: "m".into(),
                    ts: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                    input: n.parse().ok()?,
                    output: 0,
                    cache_write: 0,
                    cache_read: 0,
                })
            })
            .collect();
        Ok((recs, (start + complete) as u64))
    }

    fn fake_cost(r: &UsageRecord) -> f64 {
        r.input as f64
    }

    #[test]
    fn incremental_reads_by_offset() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("C--x");
        std::fs::create_dir_all(proj.join("sub")).unwrap();
        let f = proj.join("s.jsonl");
        std::fs::write(&f, "a,1\nb,2\nc,").unwrap();
        std::fs::write(proj.join("sub").join("t.jsonl"), "z,5\n").unwrap();
        std::fs::write(proj.join("notes.txt"), "x,1\n").unwrap();
        let db = Mutex::new(Db::open_in_memory().unwrap());

        assert_eq!(list_transcripts(dir.path()).len(), 2);
        assert_eq!(catch_up(&db, dir.path(), fake_read, fake_cost), 3);
        // de novo, sem mudanças: nada
        assert_eq!(catch_up(&db, dir.path(), fake_read, fake_cost), 0);
        let off = db.lock().unwrap().get_offset(&f.to_string_lossy()).unwrap().unwrap();
        assert_eq!(off.offset, 8);

        // linha parcial completada + nova linha
        std::fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"3\nd,4\n").unwrap();
        assert_eq!(process_file(&db, &f, fake_read, fake_cost).unwrap(), 2);
        let db = db.lock().unwrap();
        let rows = db.usage_for_session("s").unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows.iter().map(|r| r.tokens.input).sum::<u64>(), 1 + 2 + 3 + 4 + 5);
    }

    #[test]
    fn watcher_picks_up_appends() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("w.jsonl");
        std::fs::write(&f, "a,1\n").unwrap();
        let db = Arc::new(Mutex::new(Db::open_in_memory().unwrap()));
        let (tx, rx) = std::sync::mpsc::channel();
        spawn(db.clone(), dir.path().to_path_buf(), fake_read, fake_cost, move || {
            let _ = tx.send(());
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("catch-up");
        std::fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"b,2\n").unwrap();
        rx.recv_timeout(Duration::from_secs(10)).expect("evento do watcher");
        assert_eq!(db.lock().unwrap().usage_for_session("s").unwrap().len(), 2);
    }
}
