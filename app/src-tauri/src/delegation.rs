//! Delegação por etapas (docs/specs/NOITE-2.md §2): worktree git por delegação, um
//! processo `claude -p` por etapa, checkpoint com aprovação, ajuste, passagem de
//! contexto (PASSAGEM.md), bloqueio por ferramenta fora da lista, cancelamento,
//! merge no fim e recuperação no restart. Sem Tauri aqui (testável com um runner falso).

use crate::ai::AiBackend;
use crate::db::Db;
use crate::model::*;
use crate::runner::{canonical, check_not_shell_script, git, git_verbose, is_git_repo, AgentRunner, RunControl};

/// Limites: a linha de comando do Windows tem ~32k caracteres e o prompt vai inteiro nela.
pub const MAX_STEPS: usize = 30;
pub const MAX_STEP_CHARS: usize = 8_000;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_FEEDBACK_CHARS: usize = 4_000;
pub const MAX_ARGV_CHARS: usize = 30_000;

/// Raiz do contexto validada: existe, é pasta, caminho canônico.
fn context_root(ctx: &ProjectContext) -> Result<PathBuf> {
    let raw = ctx.root_path.trim();
    if raw.is_empty() {
        bail!("O contexto {} não tem pasta raiz.", ctx.name);
    }
    let root = canonical(Path::new(raw))?;
    if !root.is_dir() {
        bail!("A pasta raiz de {} não é uma pasta: {}", ctx.name, root.display());
    }
    Ok(root)
}

/// O worktree tem que estar em `<root>\.nyx\worktrees\<id>` (id hex), nada fora disso.
/// Pasta dos worktrees, fora da árvore dos projetos: `NYX_WORKTREES_DIR`, ou
/// `$NYX_DATA_DIR\worktrees`, ou `%LOCALAPPDATA%\Nyx\worktrees`.
pub fn default_worktrees_root() -> PathBuf {
    if let Some(d) = std::env::var_os("NYX_WORKTREES_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    if std::env::var_os("NYX_DATA_DIR").is_some_and(|d| !d.is_empty()) {
        return nyx_claude::data_dir().join("worktrees");
    }
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Nyx").join("worktrees")
}

fn safe_segment(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn worktree_path_for(wt_root: &Path, r: &Rec) -> Result<PathBuf> {
    if !r.d.id.chars().all(|c| c.is_ascii_hexdigit()) || !safe_segment(&r.d.id) {
        bail!("id de delegação inválido");
    }
    if !safe_segment(&r.d.context_id) {
        bail!("id de contexto inválido para pasta de worktree");
    }
    Ok(wt_root.join(&r.d.context_id).join(&r.d.id))
}

/// O worktree tem que ser `<worktrees>\<context-id>\<deleg-id>` (ou, para delegações
/// antigas, `<root>\.nyx\worktrees\<deleg-id>`). Qualquer outro caminho é recusado.
fn checked_worktree(wt_root: &Path, root: &Path, r: &Rec) -> Result<PathBuf> {
    let expected = worktree_path_for(wt_root, r)?;
    let wt = canonical(Path::new(&r.d.worktree_path))?;
    let ok_new = canonical(&expected).map(|e| e == wt).unwrap_or(false);
    let ok_legacy = canonical(&root.join(".nyx").join("worktrees").join(&r.d.id)).map(|e| e == wt).unwrap_or(false);
    if !ok_new && !ok_legacy {
        bail!("O worktree {} está fora de {}. Recusado.", wt.display(), expected.display());
    }
    Ok(wt)
}
use anyhow::{anyhow, bail, Context as _, Result};
use chrono::{DateTime, Utc};
use nyx_claude::delegate::{self as dg, StepOutcome, StepRequest};
use rusqlite::{params, Connection, OptionalExtension};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// ---------- persistência ----------

/// Delegação + campos internos (não vão para a UI).
#[derive(Debug, Clone, PartialEq)]
pub struct Rec {
    pub d: Delegation,
    pub use_ai: bool,
    pub model: Option<String>,
    /// A próxima execução começa sessão nova lendo o PASSAGEM.md.
    pub pending_handoff: bool,
    /// `finish_delegation` já foi chamado (sai da lista padrão).
    pub finalized: bool,
}

fn rfc(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn rfc_ms(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn insert(conn: &Connection, r: &Rec) -> Result<()> {
    let d = &r.d;
    conn.execute(
        "INSERT INTO delegations (id, context_id, title, status, current_step, worktree_path, branch, base_branch,
            claude_session_id, handoffs, pending_handoff, blocked_reason, use_ai, model, finalized, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            d.id, d.context_id, d.title, d.status.as_str(), d.current_step, d.worktree_path, d.branch, d.base_branch,
            d.claude_session_id, d.handoffs, r.pending_handoff as i64, d.blocked_reason, r.use_ai as i64, r.model,
            r.finalized as i64, d.created_at, d.updated_at
        ],
    )?;
    for s in &d.steps {
        save_step(conn, &d.id, s)?;
    }
    Ok(())
}

/// Grava o cabeçalho. Não sobrescreve uma delegação já cancelada (a menos que a
/// gravação seja dela mesma como cancelada). Devolve se gravou.
fn save_header(conn: &Connection, r: &Rec) -> Result<bool> {
    let d = &r.d;
    let n = conn.execute(
        "UPDATE delegations SET status = ?2, current_step = ?3, worktree_path = ?4, branch = ?5, base_branch = ?6,
            claude_session_id = ?7, handoffs = ?8, pending_handoff = ?9, blocked_reason = ?10, finalized = ?11,
            updated_at = ?12
         WHERE id = ?1 AND (status != 'cancelled' OR ?2 = 'cancelled')",
        params![
            d.id, d.status.as_str(), d.current_step, d.worktree_path, d.branch, d.base_branch, d.claude_session_id,
            d.handoffs, r.pending_handoff as i64, d.blocked_reason, r.finalized as i64, d.updated_at
        ],
    )?;
    Ok(n == 1)
}

/// Transição atômica de status: só um chamador ganha (dois cliques, toast + painel).
fn claim(conn: &Connection, id: &str, from: &[DelegationStatus], to: DelegationStatus) -> Result<bool> {
    let list = from.iter().map(|s| format!("'{}'", s.as_str())).collect::<Vec<_>>().join(",");
    let n = conn.execute(
        &format!("UPDATE delegations SET status = ?2 WHERE id = ?1 AND status IN ({list})"),
        params![id, to.as_str()],
    )?;
    Ok(n == 1)
}

fn save_step(conn: &Connection, id: &str, s: &DelegationStep) -> Result<()> {
    conn.execute(
        "INSERT INTO delegation_steps (delegation_id, idx, prompt, status, attempts, report, ai_summary, feedback,
            commit_hash, cost_usd, context_fill, started_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(delegation_id, idx) DO UPDATE SET prompt = excluded.prompt, status = excluded.status,
            attempts = excluded.attempts, report = excluded.report, ai_summary = excluded.ai_summary,
            feedback = excluded.feedback, commit_hash = excluded.commit_hash, cost_usd = excluded.cost_usd,
            context_fill = excluded.context_fill, started_at = excluded.started_at, finished_at = excluded.finished_at",
        params![
            id,
            s.index,
            s.prompt,
            s.status.as_str(),
            s.attempts,
            s.report.as_ref().map(serde_json::to_string).transpose()?,
            s.ai_summary,
            serde_json::to_string(&s.feedback)?,
            s.commit,
            s.cost_usd,
            s.context_fill,
            s.started_at,
            s.finished_at
        ],
    )?;
    Ok(())
}

fn load_steps(conn: &Connection, id: &str) -> Result<Vec<DelegationStep>> {
    let mut stmt = conn.prepare_cached(
        "SELECT idx, prompt, status, attempts, report, ai_summary, feedback, commit_hash, cost_usd, context_fill,
                started_at, finished_at
         FROM delegation_steps WHERE delegation_id = ?1 ORDER BY idx",
    )?;
    let rows = stmt.query_map([id], |r| {
        let status: String = r.get(2)?;
        let report: Option<String> = r.get(4)?;
        let feedback: String = r.get(6)?;
        Ok(DelegationStep {
            index: r.get(0)?,
            prompt: r.get(1)?,
            status: StepStatus::parse(&status),
            attempts: r.get(3)?,
            report: report.and_then(|s| serde_json::from_str(&s).ok()),
            ai_summary: r.get(5)?,
            feedback: serde_json::from_str(&feedback).unwrap_or_default(),
            commit: r.get(7)?,
            cost_usd: r.get(8)?,
            context_fill: r.get(9)?,
            live_activity: None,
            started_at: r.get(10)?,
            finished_at: r.get(11)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

const SELECT_REC: &str = "SELECT id, context_id, title, status, current_step, worktree_path, branch, base_branch,
    claude_session_id, handoffs, pending_handoff, blocked_reason, use_ai, model, finalized, created_at, updated_at
    FROM delegations";

fn map_rec(r: &rusqlite::Row<'_>) -> rusqlite::Result<Rec> {
    let status: String = r.get(3)?;
    Ok(Rec {
        d: Delegation {
            id: r.get(0)?,
            context_id: r.get(1)?,
            title: r.get(2)?,
            status: DelegationStatus::parse(&status),
            current_step: r.get(4)?,
            steps: vec![],
            worktree_path: r.get(5)?,
            branch: r.get(6)?,
            base_branch: r.get(7)?,
            claude_session_id: r.get(8)?,
            handoffs: r.get(9)?,
            blocked_reason: r.get(11)?,
            cost_usd: 0.0,
            created_at: r.get(15)?,
            updated_at: r.get(16)?,
        },
        pending_handoff: r.get::<_, i64>(10)? != 0,
        use_ai: r.get::<_, i64>(12)? != 0,
        model: r.get(13)?,
        finalized: r.get::<_, i64>(14)? != 0,
    })
}

fn fill(conn: &Connection, mut rec: Rec) -> Result<Rec> {
    rec.d.steps = load_steps(conn, &rec.d.id)?;
    rec.d.cost_usd = rec.d.steps.iter().map(|s| s.cost_usd).sum();
    Ok(rec)
}

pub fn load(conn: &Connection, id: &str) -> Result<Option<Rec>> {
    let rec = conn
        .query_row(&format!("{SELECT_REC} WHERE id = ?1"), [id], map_rec)
        .optional()?;
    rec.map(|r| fill(conn, r)).transpose()
}

pub fn list(conn: &Connection, context_id: Option<&str>, include_finished: bool) -> Result<Vec<Rec>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_REC} WHERE (?1 IS NULL OR context_id = ?1) ORDER BY updated_at DESC, created_at DESC"
    ))?;
    let recs: Vec<Rec> = stmt.query_map(params![context_id], map_rec)?.collect::<rusqlite::Result<_>>()?;
    recs.into_iter()
        .filter(|r| include_finished || !(r.finalized || r.d.status == DelegationStatus::Cancelled))
        .map(|r| fill(conn, r))
        .collect()
}

// ---------- notificações para fora (Tauri) ----------

pub trait DelegSink: Send + Sync {
    /// Qualquer mudança, inclusive `liveActivity`.
    fn changed(&self, d: &Delegation);
    /// Etapa concluída esperando Aprovar / Ajustar.
    fn checkpoint(&self, d: &Delegation, step: &DelegationStep);
    fn blocked(&self, d: &Delegation);
    fn failed(&self, d: &Delegation, message: &str);
    fn done(&self, d: &Delegation);
}

// ---------- motor ----------

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    Normal,
    Adjust(String),
    Retry,
}

pub type ExeFinder = Box<dyn Fn() -> Option<PathBuf> + Send + Sync>;

pub struct Engine {
    db: Arc<Mutex<Db>>,
    runner: Arc<dyn AgentRunner>,
    ai: Arc<dyn AiBackend>,
    sink: Arc<dyn DelegSink>,
    claude_exe: ExeFinder,
    live: Mutex<HashMap<String, String>>,
    runs: Mutex<HashMap<String, Arc<RunControl>>>,
    start_lock: Mutex<()>,
    wt_root: Mutex<PathBuf>,
    last_update_ms: Mutex<i64>,
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn clip(s: &str, max: usize) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() <= max {
        line.to_string()
    } else {
        format!("{}...", line.chars().take(max).collect::<String>())
    }
}

/// Atividade ao vivo legível a partir de um evento do stream.
pub fn activity_for(ev: &dg::StreamEvent) -> Option<String> {
    match ev {
        dg::StreamEvent::ToolUse { name, summary } => {
            let s = summary.clone().unwrap_or_default();
            let verb = match name.as_str() {
                "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => "Editando",
                "Read" => "Lendo",
                "Bash" | "PowerShell" => "Rodando",
                "Grep" | "Glob" => "Procurando",
                "WebFetch" | "WebSearch" => "Pesquisando",
                "Task" | "Agent" => "Delegando",
                _ => return Some(format!("{name} {s}").trim().to_string()),
            };
            Some(format!("{verb} {s}").trim().to_string())
        }
        dg::StreamEvent::Init { .. } => Some("Iniciando a sessão".into()),
        _ => None,
    }
}

/// Motivo do bloqueio por permissão, se o stream tiver um (ferramenta fora da lista).
pub fn detect_block(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|l| dg::detect_permission_block_line(l))
}

/// `--allowedTools "Read,Edit,Bash(npm test:*)"` no fim (um argumento, sem shell).
pub fn with_allowed_tools(mut args: Vec<String>, tools: &[String]) -> Vec<String> {
    let tools = unique_tools(tools);
    if !tools.is_empty() {
        args.push("--allowedTools".into());
        args.push(tools.join(","));
    }
    args
}

fn commit_message(step: &DelegationStep, total: usize) -> String {
    format!("nyx: step {}/{} - {}", step.index + 1, total, clip(&step.prompt, 60))
}

impl Engine {
    pub fn new(
        db: Arc<Mutex<Db>>,
        runner: Arc<dyn AgentRunner>,
        ai: Arc<dyn AiBackend>,
        sink: Arc<dyn DelegSink>,
        claude_exe: ExeFinder,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            runner,
            ai,
            sink,
            claude_exe,
            live: Mutex::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            start_lock: Mutex::new(()),
            wt_root: Mutex::new(default_worktrees_root()),
            last_update_ms: Mutex::new(0),
        })
    }

    /// Testes apontam os worktrees para uma pasta temporária.
    pub fn set_worktrees_root(&self, p: PathBuf) {
        *self.wt_root.lock().unwrap() = p;
    }

    fn wt_root(&self) -> PathBuf {
        self.wt_root.lock().unwrap().clone()
    }

    fn worktree(&self, r: &Rec) -> Result<PathBuf> {
        let ctx = self.context(&r.d.context_id)?;
        checked_worktree(&self.wt_root(), &context_root(&ctx)?, r)
    }

    /// `updatedAt` estritamente crescente (ms): a UI descarta eventos mais antigos.
    fn next_updated_at(&self) -> String {
        let mut last = self.last_update_ms.lock().unwrap();
        let ms = now().timestamp_millis().max(*last + 1);
        *last = ms;
        rfc_ms(&DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_else(now))
    }

    fn rec(&self, id: &str) -> Result<Rec> {
        load(self.db.lock().unwrap().conn(), id)?.ok_or_else(|| anyhow!("Delegação não encontrada: {id}"))
    }

    /// Delegação para a UI, com a atividade ao vivo na etapa atual.
    fn view(&self, mut r: Rec) -> Delegation {
        if let Some(a) = self.live.lock().unwrap().get(&r.d.id) {
            let cur = r.d.current_step as usize;
            if let Some(s) = r.d.steps.get_mut(cur).filter(|s| s.status == StepStatus::Running) {
                s.live_activity = Some(a.clone());
            }
        }
        r.d
    }

    fn persist(&self, r: &mut Rec) -> Result<Delegation> {
        r.d.updated_at = self.next_updated_at();
        {
            let db = self.db.lock().unwrap();
            if !save_header(db.conn(), r)? {
                // cancelada no meio do caminho: vale o que está no banco
                let cur = load(db.conn(), &r.d.id)?.ok_or_else(|| anyhow!("delegação sumiu"))?;
                drop(db);
                *r = cur;
                return Ok(self.view(r.clone()));
            }
            for s in &r.d.steps {
                save_step(db.conn(), &r.d.id, s)?;
            }
        }
        r.d.cost_usd = r.d.steps.iter().map(|s| s.cost_usd).sum();
        let v = self.view(r.clone());
        self.sink.changed(&v);
        Ok(v)
    }

    pub fn get(&self, id: &str) -> Result<Delegation> {
        Ok(self.view(self.rec(id)?))
    }

    pub fn list(&self, context_id: Option<&str>, include_finished: bool) -> Result<Vec<Delegation>> {
        let recs = list(self.db.lock().unwrap().conn(), context_id, include_finished)?;
        Ok(recs.into_iter().map(|r| self.view(r)).collect())
    }

    pub fn active_count(&self) -> Result<u64> {
        let recs = list(self.db.lock().unwrap().conn(), None, false)?;
        Ok(recs.iter().filter(|r| r.d.status.is_active() && r.d.status != DelegationStatus::Failed).count() as u64)
    }

    fn context(&self, id: &str) -> Result<ProjectContext> {
        self.db
            .lock()
            .unwrap()
            .get_context(id)?
            .ok_or_else(|| anyhow!("Contexto não encontrado. Crie ou escolha um contexto antes de delegar."))
    }

    fn check_tools(ctx: &ProjectContext) -> Result<()> {
        if ctx.allowed_tools.iter().all(|t| t.trim().is_empty()) {
            bail!("O contexto {} não tem ferramentas permitidas. Defina a lista em Contextos antes de delegar.", ctx.name);
        }
        Ok(())
    }

    pub fn create(&self, draft: DelegationDraft) -> Result<Delegation> {
        let ctx = self.context(&draft.context_id)?;
        let steps: Vec<String> =
            draft.steps.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if steps.is_empty() {
            bail!("A delegação precisa de pelo menos uma etapa.");
        }
        if steps.len() > MAX_STEPS {
            bail!("No máximo {MAX_STEPS} etapas por delegação.");
        }
        if let Some(i) = steps.iter().position(|s| s.chars().count() > MAX_STEP_CHARS) {
            bail!("A etapa {} passa de {MAX_STEP_CHARS} caracteres. Encurte ou divida.", i + 1);
        }
        let title = draft.title.trim();
        if title.is_empty() {
            bail!("A delegação precisa de um título.");
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            bail!("O título passa de {MAX_TITLE_CHARS} caracteres.");
        }
        if let Some(m) = draft.model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
            let ok = !m.starts_with('-') && m.chars().all(|c| c.is_ascii_alphanumeric() || "._-[]".contains(c));
            if !ok {
                bail!("Nome de modelo inválido: {m}");
            }
        }
        let root = context_root(&ctx)?;
        if is_git_repo(&root) && git(&root, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
            bail!("O repositório de {} ainda não tem nenhum commit, Dev. Faça o primeiro commit e me chame de novo.", ctx.name);
        }
        if !is_git_repo(&root) {
            bail!(
                "A pasta de {} não é um repositório git. A delegação usa um worktree por tarefa; rode git init lá primeiro.",
                ctx.name
            );
        }
        Self::check_tools(&ctx)?;
        let b: [u8; 6] = rand::random();
        let id = hex::encode(b);
        let ts = rfc(&now());
        let rec = Rec {
            d: Delegation {
                id: id.clone(),
                context_id: ctx.id.clone(),
                title: title.to_string(),
                status: DelegationStatus::Draft,
                current_step: 0,
                steps: steps
                    .into_iter()
                    .enumerate()
                    .map(|(i, p)| DelegationStep {
                        index: i as u32,
                        prompt: p,
                        status: StepStatus::Pending,
                        attempts: 0,
                        report: None,
                        ai_summary: None,
                        feedback: vec![],
                        commit: None,
                        cost_usd: 0.0,
                        context_fill: None,
                        live_activity: None,
                        started_at: None,
                        finished_at: None,
                    })
                    .collect(),
                worktree_path: String::new(),
                branch: format!("nyx/deleg-{id}"),
                base_branch: String::new(),
                claude_session_id: None,
                handoffs: 0,
                blocked_reason: None,
                cost_usd: 0.0,
                created_at: ts.clone(),
                updated_at: ts,
            },
            use_ai: draft.use_ai,
            model: draft.model.filter(|m| !m.trim().is_empty()),
            pending_handoff: false,
            finalized: false,
        };
        insert(self.db.lock().unwrap().conn(), &rec)?;
        let v = self.view(rec);
        self.sink.changed(&v);
        Ok(v)
    }

    fn ensure_single_active(&self, r: &Rec) -> Result<()> {
        let others = list(self.db.lock().unwrap().conn(), Some(&r.d.context_id), false)?;
        if let Some(o) = others.iter().find(|o| o.d.id != r.d.id && o.d.status.is_active()) {
            bail!("Já existe uma delegação ativa neste contexto: {}. Finalize ou cancele antes.", o.d.title);
        }
        Ok(())
    }

    fn create_worktree(&self, r: &mut Rec, ctx: &ProjectContext) -> Result<()> {
        let root = context_root(ctx)?;
        if git(&root, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
            bail!("O repositório de {} ainda não tem nenhum commit, Dev. Faça o primeiro commit e me chame de novo.", ctx.name);
        }
        let base = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        if base == "HEAD" {
            bail!("O repositório de {} está em HEAD destacado. Troque para uma branch antes de delegar.", ctx.name);
        }
        let path = worktree_path_for(&self.wt_root(), r)?;
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let path_s = path.to_string_lossy().to_string();
        let add = |_: ()| {
            git_verbose(&root, &["worktree", "add", "-b", &r.d.branch, &path_s, &base])
                .map_err(|out| anyhow!("git worktree add: {}", out.trim()))
        };
        if let Err(e) = add(()) {
            // Sobra de uma tentativa anterior (rascunho nunca rodou): limpa e tenta de novo.
            if format!("{e:#}").contains("already exists") {
                log::info!("worktree/branch órfãos de {}: limpando", r.d.id);
                cleanup_worktree(&root, &path, &r.d.branch);
                add(()).context("criando o worktree da delegação (depois de limpar sobras)")?;
            } else {
                return Err(e).context("criando o worktree da delegação");
            }
        }
        r.d.base_branch = base;
        r.d.worktree_path = path_s;
        checked_worktree(&self.wt_root(), &root, r)?;
        Ok(())
    }

    /// Começa (rascunho) ou refaz a etapa atual (bloqueada/falhou).
    fn claim(&self, id: &str, from: &[DelegationStatus], to: DelegationStatus) -> Result<bool> {
        claim(self.db.lock().unwrap().conn(), id, from, to)
    }

    pub fn start(self: &Arc<Self>, id: &str) -> Result<Delegation> {
        use DelegationStatus as S;
        let (mut r, ctx) = {
            // checagem de "1 ativa por contexto" + reserva sem janela entre as duas
            let _g = self.start_lock.lock().unwrap();
            let r = self.rec(id)?;
            let ctx = self.context(&r.d.context_id)?;
            Self::check_tools(&ctx)?;
            if !matches!(r.d.status, S::Draft | S::Blocked | S::Failed) {
                bail!("Não dá para iniciar uma delegação em {}.", r.d.status.as_str());
            }
            self.ensure_single_active(&r)?;
            if !self.claim(id, &[r.d.status], S::Running)? {
                bail!("Essa delegação já está sendo iniciada.");
            }
            (r, ctx)
        };
        match r.d.status {
            S::Draft => {
                if let Err(e) = self.create_worktree(&mut r, &ctx) {
                    let _ = self.claim(id, &[S::Running], S::Draft);
                    return Err(e);
                }
                let (wt, branch) = (PathBuf::from(&r.d.worktree_path), r.d.branch.clone());
                match self.begin_step(r, Mode::Normal) {
                    Ok(v) => Ok(v),
                    Err(e) => {
                        // a persistência falhou: não deixa worktree/branch órfãos
                        if let Ok(root) = context_root(&ctx) {
                            cleanup_worktree(&root, &wt, &branch);
                        }
                        let _ = self.claim(id, &[S::Running], S::Draft);
                        Err(e)
                    }
                }
            }
            _ => self.begin_step(r, Mode::Retry),
        }
    }

    pub fn approve(self: &Arc<Self>, id: &str) -> Result<Delegation> {
        let mut r = self.rec(id)?;
        // `blocked`: "aprovar assim mesmo" (commita o que houver e segue).
        // `failed`: tenta o commit de novo (ex.: depois de corrigir o pre-commit), sem rodar a etapa.
        let from = r.d.status;
        if !matches!(from, DelegationStatus::Checkpoint | DelegationStatus::Blocked | DelegationStatus::Failed) {
            bail!("Nada para aprovar: a delegação está em {}.", r.d.status.as_str());
        }
        // Reserva atômica antes de qualquer efeito: o segundo clique perde aqui.
        if !self.claim(id, &[from], DelegationStatus::Running)? {
            bail!("Essa etapa já foi aprovada.");
        }
        let committed = (|| -> Result<Option<String>> {
            let wt = self.worktree(&r)?;
            // O PASSAGEM.md (e settings locais do Claude) nunca entram no commit do usuário.
            git(&wt, &["add", "-A", "--", ".", ":(exclude)PASSAGEM.md", ":(exclude).claude/settings.local.json"])?;
            let staged = git(&wt, &["diff", "--cached", "--quiet"]).is_err();
            if !staged {
                return Ok(None);
            }
            let cur = r.d.current_step as usize;
            let msg = commit_message(&r.d.steps[cur], r.d.steps.len());
            if git(&wt, &["config", "user.name"]).is_err() || git(&wt, &["config", "user.email"]).is_err() {
                bail!(
                    "Dev, o git não sabe quem você é neste repositório. Defina user.name e user.email \
                     (git config --global ...) e aprove de novo."
                );
            }
            if let Err(out) = git_verbose(&wt, &["commit", "-q", "-m", &msg]) {
                let head: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).take(5).collect();
                bail!(
                    "O commit da etapa falhou, provavelmente num hook pre-commit. Nada foi perdido; corrija e aprove de novo.\n{}",
                    head.join("\n")
                );
            }
            Ok(Some(git(&wt, &["rev-parse", "--short", "HEAD"])?))
        })();
        let hash = match committed {
            Ok(h) => h,
            Err(e) => {
                // Falha de git: fica `failed` com o motivo (1ª linha = resumo, resto = saída crua);
                // Aprovar de novo tenta só o commit.
                r.d.status = DelegationStatus::Failed;
                r.d.blocked_reason = Some(format!("{e:#}"));
                let v = self.persist(&mut r)?;
                self.sink.failed(&v, r.d.blocked_reason.as_deref().unwrap_or(""));
                return Ok(v);
            }
        };
        let cur = r.d.current_step as usize;
        let total = r.d.steps.len();
        {
            let s = &mut r.d.steps[cur];
            s.status = StepStatus::Approved;
            s.commit = hash;
        }
        r.d.blocked_reason = None;
        if cur + 1 >= total {
            r.d.status = DelegationStatus::Done;
            let v = self.persist(&mut r)?;
            self.sink.done(&v);
            Ok(v)
        } else {
            r.d.current_step += 1;
            self.begin_step(r, Mode::Normal)
        }
    }

    pub fn adjust(self: &Arc<Self>, id: &str, feedback: &str) -> Result<Delegation> {
        let fb = feedback.trim();
        if fb.is_empty() {
            bail!("Escreva o ajuste que você quer.");
        }
        if fb.chars().count() > MAX_FEEDBACK_CHARS {
            bail!("O ajuste passa de {MAX_FEEDBACK_CHARS} caracteres. Resuma.");
        }
        let mut r = self.rec(id)?;
        if !matches!(r.d.status, DelegationStatus::Checkpoint | DelegationStatus::Blocked | DelegationStatus::Failed) {
            bail!("Não dá para ajustar uma delegação em {}.", r.d.status.as_str());
        }
        if r.d.status != DelegationStatus::Checkpoint {
            let ctx = self.context(&r.d.context_id)?;
            Self::check_tools(&ctx)?;
        }
        if !self.claim(id, &[r.d.status], DelegationStatus::Running)? {
            bail!("Essa etapa já está sendo refeita.");
        }
        let cur = r.d.current_step as usize;
        r.d.steps[cur].feedback.push(fb.to_string());
        self.begin_step(r, Mode::Adjust(fb.to_string()))
    }

    pub fn cancel(&self, id: &str) -> Result<Delegation> {
        let mut r = self.rec(id)?;
        if matches!(r.d.status, DelegationStatus::Done | DelegationStatus::Cancelled) {
            bail!("A delegação já está {}.", if r.d.status == DelegationStatus::Done { "concluída" } else { "cancelada" });
        }
        if let Some(ctl) = self.runs.lock().unwrap().remove(id) {
            ctl.cancel();
        }
        self.live.lock().unwrap().remove(id);
        let cur = r.d.current_step as usize;
        if let Some(s) = r.d.steps.get_mut(cur).filter(|s| s.status == StepStatus::Running) {
            s.status = StepStatus::Failed;
            s.finished_at = Some(rfc(&now()));
            s.report = Some(StepReport { title: "Cancelada".into(), lines: vec![], truncated: false });
        }
        r.d.status = DelegationStatus::Cancelled;
        self.persist(&mut r)
    }

    pub fn finish(&self, id: &str, merge: bool) -> Result<Delegation> {
        let mut r = self.rec(id)?;
        if r.finalized {
            bail!("A delegação já foi finalizada.");
        }
        match (r.d.status, merge) {
            (DelegationStatus::Done, _) | (DelegationStatus::Cancelled, false) => {}
            (s, _) => bail!("Só dá para finalizar uma delegação concluída (está em {}).", s.as_str()),
        }
        if merge {
            let ctx = self.context(&r.d.context_id)?;
            let root = context_root(&ctx)?;
            let wt = checked_worktree(&self.wt_root(), &root, &r)?;
            if git(&root, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_ok() {
                bail!("Já existe um merge em andamento na pasta do contexto. Conclua ou aborte esse merge antes; a Nyx não mexe nele.");
            }
            if !git(&root, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
                bail!("A pasta do contexto tem alterações não commitadas. Commite ou guarde antes do merge.");
            }
            let head = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
            if head != r.d.base_branch {
                bail!(
                    "A pasta do contexto está em {head}, não em {}. Troque para {} e tente de novo.",
                    r.d.base_branch,
                    r.d.base_branch
                );
            }
            let msg = format!("Merge Nyx delegation: {}", r.d.title);
            if let Err(e) = git(&root, &["merge", "--no-ff", "-m", &msg, &r.d.branch]) {
                // só desfaz o merge que esta chamada começou (antes não havia MERGE_HEAD)
                if git(&root, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_ok() {
                    let _ = git(&root, &["merge", "--abort"]);
                }
                bail!("O merge falhou e foi desfeito ({e}). A branch {} continua lá.", r.d.branch);
            }
            if let Err(e) = git(&root, &["worktree", "remove", "--force", &wt.to_string_lossy()]) {
                log::warn!("worktree remove: {e:#}");
            }
        }
        r.finalized = true;
        self.persist(&mut r)
    }

    /// No start do app: etapa `running` sem processo vira `failed` (dá para refazer).
    pub fn recover(&self) -> Result<usize> {
        let recs = list(self.db.lock().unwrap().conn(), None, true)?;
        let mut n = 0;
        for mut r in recs {
            if !matches!(r.d.status, DelegationStatus::Running | DelegationStatus::Handoff) {
                continue;
            }
            let cur = r.d.current_step as usize;
            if let Some(s) = r.d.steps.get_mut(cur) {
                s.status = StepStatus::Failed;
                s.finished_at = Some(rfc(&now()));
                s.report = Some(StepReport {
                    title: "Interrompida".into(),
                    lines: vec!["A Nyx fechou durante a etapa. Use Refazer ou Ajustar.".into()],
                    truncated: false,
                });
            }
            r.d.status = DelegationStatus::Failed;
            self.persist(&mut r)?;
            n += 1;
        }
        Ok(n)
    }

    fn begin_step(self: &Arc<Self>, mut r: Rec, mode: Mode) -> Result<Delegation> {
        let cur = r.d.current_step as usize;
        {
            let s = &mut r.d.steps[cur];
            s.status = StepStatus::Running;
            s.attempts += 1;
            s.started_at = Some(rfc(&now()));
            s.finished_at = None;
        }
        r.d.status = DelegationStatus::Running;
        r.d.blocked_reason = None;
        let ctl = Arc::new(RunControl::default());
        self.runs.lock().unwrap().insert(r.d.id.clone(), ctl.clone());
        let v = self.persist(&mut r)?;
        let me = self.clone();
        let id = r.d.id.clone();
        std::thread::Builder::new().name(format!("nyx-deleg-{id}")).spawn(move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| me.execute(&id, mode, &ctl)))
                .unwrap_or_else(|_| Err(anyhow!("erro interno na etapa (panic)")));
            if let Err(e) = res {
                if !ctl.is_cancelled() {
                    me.fail(&id, &format!("{e:#}"));
                }
            }
            me.runs.lock().unwrap().retain(|_, c| !Arc::ptr_eq(c, &ctl));
            me.live.lock().unwrap().remove(&id);
        })?;
        Ok(v)
    }

    /// Ao sair do app: mata os `claude` em execução (a etapa vira `failed` no próximo start).
    pub fn kill_all(&self) -> usize {
        let runs: Vec<Arc<RunControl>> = self.runs.lock().unwrap().drain().map(|(_, c)| c).collect();
        for c in &runs {
            c.cancel();
        }
        runs.len()
    }

    fn fail(&self, id: &str, msg: &str) {
        let Ok(mut r) = self.rec(id) else { return };
        let cur = r.d.current_step as usize;
        if let Some(s) = r.d.steps.get_mut(cur) {
            s.status = StepStatus::Failed;
            s.finished_at = Some(rfc(&now()));
            s.report = Some(StepReport { title: "Etapa falhou".into(), lines: vec![clip(msg, 280)], truncated: false });
        }
        r.d.status = DelegationStatus::Failed;
        if let Ok(v) = self.persist(&mut r) {
            self.sink.failed(&v, msg);
        }
    }

    /// Roda o agente e devolve o resultado + as ferramentas negadas.
    fn run_agent(&self, id: &str, req: &StepRequest, tools: &[String], ctl: &RunControl) -> Result<(StepOutcome, Option<String>)> {
        let exe = (self.claude_exe)()
            .ok_or_else(|| anyhow!("Claude Code não encontrado no PATH. Instale ou defina NYX_CLAUDE_EXE."))?;
        check_not_shell_script(&exe)?;
        let mut sc = dg::build_command(req, &exe);
        // `--allowedTools` sempre por último.
        sc.args = with_allowed_tools(std::mem::take(&mut sc.args), tools);
        let total: usize = sc.args.iter().map(|a| a.chars().count() + 3).sum();
        if total > MAX_ARGV_CHARS {
            bail!("A lista de ferramentas ficou grande demais para a linha de comando ({total} caracteres).");
        }
        let mut on_line = |line: &str| {
            for ev in dg::parse_stream_line(line) {
                if let Some(a) = activity_for(&ev) {
                    self.live.lock().unwrap().insert(id.to_string(), a);
                    if let Ok(r) = self.rec(id) {
                        self.sink.changed(&self.view(r));
                    }
                }
            }
        };
        let lines = self.runner.run(&sc, ctl, &mut on_line).context("rodando o Claude Code")?;
        let outcome = dg::parse_stream(std::io::Cursor::new(lines.join("\n")))?;
        Ok((outcome, detect_block(&lines)))
    }

    fn execute(&self, id: &str, mode: Mode, ctl: &RunControl) -> Result<()> {
        let r = self.rec(id)?;
        let ctx = self.context(&r.d.context_id)?;
        let cur = r.d.current_step as usize;
        let step = r.d.steps[cur].clone();
        let worktree = checked_worktree(&self.wt_root(), &context_root(&ctx)?, &r)?;
        let prompt = match &mode {
            Mode::Normal | Mode::Retry => step.prompt.clone(),
            Mode::Adjust(fb) => format!(
                "Ajuste pedido pelo usuário para a etapa atual:\n{fb}\n\nEtapa (refaça considerando o ajuste):\n{}",
                step.prompt
            ),
        };
        let mut req = StepRequest {
            cwd: worktree,
            prompt,
            context_instructions: Some(ctx.instructions.clone()),
            references: ctx.references.clone(),
            resume_session: r.d.claude_session_id.clone(),
            model: r.model.clone(),
            permission_mode: None,
        };
        let fresh = r.pending_handoff;
        if fresh {
            req = dg::handoff_resume_request(&req);
        }
        let (outcome, block) = self.run_agent(id, &req, &ctx.allowed_tools, ctl)?;
        if ctl.is_cancelled() {
            return Ok(());
        }

        let mut r = self.rec(id)?;
        if r.d.status == DelegationStatus::Cancelled {
            return Ok(());
        }
        if fresh {
            r.pending_handoff = false;
        }
        if !outcome.session_id.is_empty() {
            r.d.claude_session_id = Some(outcome.session_id.clone());
        }
        {
            let s = &mut r.d.steps[cur];
            s.cost_usd += outcome.total_cost_usd.unwrap_or(0.0);
            s.context_fill = outcome.context_fill;
        }

        if let Some(reason) = block {
            let s = &mut r.d.steps[cur];
            s.status = StepStatus::Failed;
            s.finished_at = Some(rfc(&now()));
            s.report = Some(StepReport {
                title: outcome.report.title.clone(),
                lines: outcome.report.lines.clone(),
                truncated: outcome.report.truncated,
            });
            r.d.status = DelegationStatus::Blocked;
            r.d.blocked_reason = Some(format!("{reason}. Adicione em Contextos e refaça, ou cancele."));
            let v = self.persist(&mut r)?;
            self.sink.blocked(&v);
            return Ok(());
        }
        if outcome.is_error {
            self.persist(&mut r)?;
            bail!("{}", clip(&outcome.result_text, 280));
        }

        // Passagem de contexto: com ≥ 60%, pede o PASSAGEM.md na mesma sessão e a
        // próxima execução começa sessão nova lendo o arquivo.
        if dg::needs_handoff(&outcome) {
            r.d.status = DelegationStatus::Handoff;
            self.persist(&mut r)?;
            let w = dg::handoff_write_request(&req, &outcome.session_id);
            let (wo, wdenied) = self.run_agent(id, &w, &ctx.allowed_tools, ctl)?;
            if ctl.is_cancelled() {
                return Ok(());
            }
            r = self.rec(id)?;
            r.d.steps[cur].cost_usd += wo.total_cost_usd.unwrap_or(0.0);
            if wo.is_error || wdenied.is_some() {
                self.persist(&mut r)?;
                bail!("A passagem de contexto (PASSAGEM.md) falhou: {}", clip(&wo.result_text, 200));
            }
            r.d.handoffs += 1;
            r.pending_handoff = true;
        }

        let ai_summary = if r.use_ai && self.ai.available() {
            // Local-first: só o relatório de checkpoint (título + até 5 linhas) e números.
            let mut report = outcome.report.title.clone();
            for l in &outcome.report.lines {
                report.push('\n');
                report.push_str(l);
            }
            let report: String = report.chars().take(1_500).collect();
            let changed = git(&req.cwd, &["status", "--short"]).map(|s| s.lines().count()).unwrap_or(0);
            let diff = format!("{changed} arquivo(s) alterado(s)");
            match self.ai.summarize_step(&report, &diff) {
                Ok((text, cost)) => {
                    let _ = self.db.lock().unwrap().add_ai_cost("summary", cost, now());
                    Some(text)
                }
                Err(e) => {
                    log::warn!("resumo da IA: {e:#}");
                    None
                }
            }
        } else {
            None
        };

        {
            let s = &mut r.d.steps[cur];
            s.status = StepStatus::Checkpoint;
            s.finished_at = Some(rfc(&now()));
            s.report = Some(StepReport {
                title: outcome.report.title.clone(),
                lines: outcome.report.lines.clone(),
                truncated: outcome.report.truncated,
            });
            s.ai_summary = ai_summary;
        }
        r.d.status = DelegationStatus::Checkpoint;
        self.live.lock().unwrap().remove(id);
        let v = self.persist(&mut r)?;
        let s = v.steps[cur].clone();
        self.sink.checkpoint(&v, &s);
        Ok(())
    }
}

/// Remove worktree e branch de uma delegação que não chegou a rodar (sobras).
fn cleanup_worktree(root: &Path, path: &Path, branch: &str) {
    let p = path.to_string_lossy().to_string();
    let _ = git(root, &["worktree", "remove", "--force", &p]);
    let _ = git(root, &["worktree", "prune"]);
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
    let _ = git(root, &["branch", "-D", branch]);
}

/// Mensagem de toast do checkpoint: "Etapa 2 de 4 concluída. Aprova?"
pub fn checkpoint_title(d: &Delegation) -> String {
    format!("Etapa {} de {} concluída. Aprova?", d.current_step + 1, d.steps.len())
}

pub fn unique_tools(tools: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    tools.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && seen.insert(t.clone())).collect()
}
