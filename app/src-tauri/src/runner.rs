//! Execução do agente de uma etapa (`claude -p ... --output-format stream-json`) e
//! comandos git, sem shell. No Windows, sem janela de console e com kill da árvore.

use std::io::BufReader;
use nyx_claude::delegate::StepCommand;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Controle de uma execução: pid para o cancelamento e a marca de cancelada.
#[derive(Default)]
pub struct RunControl {
    pid: Mutex<Option<u32>>,
    cancelled: AtomicBool,
}

impl RunControl {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Marca como cancelada e mata a árvore de processos, se houver um rodando.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(pid) = *self.pid.lock().unwrap() {
            kill_tree(pid);
        }
    }

    pub fn set_pid(&self, pid: Option<u32>) {
        *self.pid.lock().unwrap() = pid;
    }
}

pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = no_window(Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

/// Quem roda o agente. O processo real em produção; um roteiro nos testes.
pub trait AgentRunner: Send + Sync {
    /// Roda `cmd` (prompt pelo stdin) e chama `on_line` a cada linha do stdout.
    /// Devolve as linhas que importam para o resultado.
    fn run(&self, cmd: &StepCommand, ctl: &RunControl, on_line: &mut dyn FnMut(&str)) -> std::io::Result<Vec<String>>;
}

/// Variáveis que nunca vão para o `claude` filho: a chave da IA da Nyx faria o Claude
/// Code cobrar na API (e o agente a veria no ambiente).
pub const SCRUBBED_ENV: &[&str] =
    &["NYX_ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "NYX_AI_BASE_URL"];

/// Etapa travada (rede, loop) é morta depois disso.
pub const STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45 * 60);

/// Linhas guardadas para o resultado: tudo menos as saídas de ferramenta (`user`),
/// que podem ser enormes, salvo as que falam de permissão.
fn keep_line(line: &str) -> bool {
    !(line.contains("\"type\":\"user\"") && !line.contains("permission"))
}

/// Processo real. `env` extra é aplicado a cada execução (os testes trocam o cenário
/// do `nyx-fakeclaude` por aqui, sem mexer no ambiente global).
#[derive(Default)]
pub struct ProcessRunner {
    pub env: Mutex<Vec<(String, String)>>,
}

impl AgentRunner for ProcessRunner {
    fn run(&self, sc: &StepCommand, ctl: &RunControl, on_line: &mut dyn FnMut(&str)) -> std::io::Result<Vec<String>> {
        use std::io::{BufRead, Write};
        if ctl.is_cancelled() {
            return Ok(vec![]);
        }
        let mut cmd = Command::new(&sc.program);
        for (k, v) in self.env.lock().unwrap().iter() {
            cmd.env(k, v);
        }
        for k in SCRUBBED_ENV {
            cmd.env_remove(k);
        }
        let mut child = no_window(&mut cmd)
            .args(&sc.args)
            .current_dir(&sc.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let pid = child.id();
        if !crate::job::assign(&child) {
            log::warn!("processo {pid} fora do job object (sem kill automático se a Nyx cair)");
        }
        ctl.set_pid(Some(pid));
        if ctl.is_cancelled() {
            kill_tree(pid);
        }
        // Prompt pelo stdin, numa thread (o filho pode escrever antes de ler tudo).
        if let Some(mut stdin) = child.stdin.take() {
            let text = sc.stdin.clone();
            std::thread::spawn(move || {
                let _ = stdin.write_all(text.as_bytes());
                // drop fecha o stdin
            });
        }
        // Watchdog sem polling: espera o fim ou o prazo.
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(STEP_TIMEOUT) {
                log::warn!("etapa passou de {} min: matando o processo {pid}", STEP_TIMEOUT.as_secs() / 60);
                kill_tree(pid);
            }
        });
        let mut lines = vec![];
        if let Some(out) = child.stdout.take() {
            let mut reader = BufReader::new(out);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf);
                        let line = line.trim_end_matches(['\r', '\n']);
                        on_line(line);
                        if keep_line(line) {
                            lines.push(line.to_string());
                        }
                    }
                }
            }
        }
        let _ = child.wait();
        ctl.set_pid(None);
        let _ = done_tx.send(());
        Ok(lines)
    }
}

/// Executável do Claude Code: `NYX_CLAUDE_EXE` ou o primeiro `claude(.exe|.cmd)` no PATH.
pub fn claude_exe() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NYX_CLAUDE_EXE") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    // Só executável nativo: um .cmd/.bat passaria pelo cmd.exe (shell), o que não fazemos.
    let names: &[&str] = if cfg!(windows) { &["claude.exe"] } else { &["claude"] };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for n in names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Recusa scripts que o Windows rodaria via `cmd.exe` (o prompt vai sempre como argv).
pub fn check_not_shell_script(exe: &std::path::Path) -> anyhow::Result<()> {
    let ext = exe.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if matches!(ext.as_str(), "cmd" | "bat" | "ps1" | "sh") {
        anyhow::bail!(
            "{} é um script ({ext}); a Nyx só roda o Claude Code como executável, sem shell. Use o claude.exe do instalador nativo.",
            exe.display()
        );
    }
    Ok(())
}

/// Caminho canônico sem o prefixo `\\?\` (que o git do Windows não entende bem).
pub fn canonical(p: &std::path::Path) -> anyhow::Result<PathBuf> {
    let c = std::fs::canonicalize(p).map_err(|e| anyhow::anyhow!("caminho inválido {}: {e}", p.display()))?;
    Ok(nyx_claude::hooks::clean_path(&c))
}

// ---------- git ----------

/// Roda `git <args>` em `cwd`. Erro com o stderr quando o código não é 0.
pub fn git(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = no_window(Command::new("git").args(args).current_dir(cwd).stdin(Stdio::null()))
        .output()
        .map_err(|e| anyhow::anyhow!("git não encontrado: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        anyhow::bail!("git {}: {}", args.first().unwrap_or(&""), msg);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Como `git`, mas devolve a saída completa (stdout + stderr) no erro, para mostrar ao
/// usuário (ex.: o que um hook pre-commit reclamou).
pub fn git_verbose(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = no_window(Command::new("git").args(args).current_dir(cwd).stdin(Stdio::null()))
        .output()
        .map_err(|e| format!("git não encontrado: {e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let mut all = String::from_utf8_lossy(&out.stdout).to_string();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    Err(all)
}

pub fn is_git_repo(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).map(|s| s == "true").unwrap_or(false)
}
