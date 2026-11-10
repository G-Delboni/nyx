//! Logger em arquivo, sem dependência nova: `data_dir()/logs/nyx.log`, rotação em 1 MB
//! para `nyx.log.1`. Nível info; `NYX_LOG=debug` (ou trace/warn/error) muda o nível.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_BYTES: u64 = 1024 * 1024;

pub struct FileLogger {
    path: PathBuf,
    max_bytes: u64,
    level: LevelFilter,
    file: Mutex<Option<(File, u64)>>,
}

fn rotated(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".1");
    PathBuf::from(s)
}

impl FileLogger {
    pub fn new(path: &Path, max_bytes: u64, level: LevelFilter) -> Self {
        Self { path: path.to_path_buf(), max_bytes, level, file: Mutex::new(None) }
    }

    fn open(&self) -> Option<(File, u64)> {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let f = OpenOptions::new().create(true).append(true).open(&self.path).ok()?;
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        Some((f, len))
    }

    fn write_line(&self, line: &str) {
        let mut guard = match self.file.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if guard.is_none() {
            *guard = self.open();
        }
        let needs_rotation = matches!(&*guard, Some((_, len)) if *len > 0 && len + line.len() as u64 > self.max_bytes);
        if needs_rotation {
            *guard = None; // fecha antes de renomear (Windows)
            let _ = std::fs::rename(&self.path, rotated(&self.path));
            *guard = self.open();
        }
        if let Some((f, len)) = guard.as_mut() {
            if f.write_all(line.as_bytes()).is_ok() {
                *len += line.len() as u64;
            }
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= self.level
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        // Só o que é da Nyx (crates de terceiros ficam de fora, a não ser em debug).
        let ours = r.target().starts_with("nyx");
        if !ours && r.level() > Level::Warn && self.level < LevelFilter::Debug {
            return;
        }
        let ts = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z");
        self.write_line(&format!("{ts} {:<5} {}: {}\n", r.level(), r.target(), r.args()));
    }

    fn flush(&self) {
        if let Ok(mut g) = self.file.lock() {
            if let Some((f, _)) = g.as_mut() {
                let _ = f.flush();
            }
        }
    }
}

pub fn level_from_env() -> LevelFilter {
    match std::env::var("NYX_LOG").unwrap_or_default().to_ascii_lowercase().as_str() {
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        "off" => LevelFilter::Off,
        _ => LevelFilter::Info,
    }
}

/// Instala o logger global (uma vez; chamadas seguintes são ignoradas).
pub fn init(path: &Path) {
    let level = level_from_env();
    // `set_boxed_logger` exige a feature "std" do log; leak de um único logger global.
    let lg: &'static FileLogger = Box::leak(Box::new(FileLogger::new(path, MAX_BYTES, level)));
    if log::set_logger(lg).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_filters_and_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("logs").join("nyx.log");
        let lg = FileLogger::new(&p, 200, LevelFilter::Info);
        let rec = |lvl, target: &'static str, msg: &'static str| {
            lg.log(&Record::builder().level(lvl).target(target).args(format_args!("{msg}")).build());
        };
        rec(Level::Info, "nyx_lib", "primeira");
        rec(Level::Debug, "nyx_lib", "debug some");
        rec(Level::Info, "hyper", "terceiro some");
        lg.flush();
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("INFO  nyx_lib: primeira"));
        assert!(!body.contains("debug some") && !body.contains("terceiro some"));
        for _ in 0..10 {
            rec(Level::Warn, "nyx_lib", "linha de enchimento para passar de 200 bytes");
        }
        lg.flush();
        let main = std::fs::metadata(&p).unwrap().len();
        assert!(main <= 200, "{main}");
        assert!(rotated(&p).exists());
    }
}
