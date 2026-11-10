//! Delegação de ponta a ponta com o `nyx-fakeclaude` (processo real, nunca o `claude`
//! de verdade) num repositório git temporário. Precisa do binário compilado:
//! `cargo build -p nyx-fakeclaude` (o `cargo test --workspace` já compila).

use crate::ai::fake::FakeAi;
use crate::ai::{ChatReply, ProposedAction};
use crate::chat::{self, ActionTarget};
use crate::db::Db;
use crate::delegation::{DelegSink, Engine};
use crate::model::*;
use crate::runner::{git, ProcessRunner};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn fake_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.parent()?; // target/debug/deps -> target/debug
    let p = dir.join(if cfg!(windows) { "nyx-fakeclaude.exe" } else { "nyx-fakeclaude" });
    p.is_file().then_some(p)
}

#[derive(Default)]
struct RecSink {
    changes: Mutex<Vec<Delegation>>,
    log: Mutex<Vec<String>>,
}

impl DelegSink for RecSink {
    fn changed(&self, d: &Delegation) {
        self.changes.lock().unwrap().push(d.clone());
    }
    fn checkpoint(&self, d: &Delegation, s: &DelegationStep) {
        self.log.lock().unwrap().push(format!("checkpoint {} {}", crate::delegation::checkpoint_title(d), s.index));
    }
    fn blocked(&self, d: &Delegation) {
        self.log.lock().unwrap().push(format!("blocked {}", d.blocked_reason.clone().unwrap_or_default()));
    }
    fn failed(&self, _: &Delegation, m: &str) {
        self.log.lock().unwrap().push(format!("failed {m}"));
    }
    fn done(&self, d: &Delegation) {
        self.log.lock().unwrap().push(format!("done {}", d.title));
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    root: PathBuf,
    db: Arc<Mutex<Db>>,
    runner: Arc<ProcessRunner>,
    sink: Arc<RecSink>,
    ai: Arc<FakeAi>,
    engine: Arc<Engine>,
    ctx: ProjectContext,
}

fn rig(tools: &[&str]) -> Option<Rig> {
    let Some(exe) = fake_exe() else {
        eprintln!("nyx-fakeclaude não compilado; rode `cargo build -p nyx-fakeclaude`. Teste pulado.");
        return None;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]).unwrap();
    git(&root, &["config", "user.email", "nyx-test@example.invalid"]).unwrap();
    git(&root, &["config", "user.name", "Nyx Test"]).unwrap();
    git(&root, &["config", "commit.gpgsign", "false"]).unwrap();
    std::fs::write(root.join("README.md"), "demo\n").unwrap();
    git(&root, &["add", "-A"]).unwrap();
    git(&root, &["commit", "-q", "-m", "init"]).unwrap();

    let db = Arc::new(Mutex::new(Db::open_in_memory().unwrap()));
    let ctx = crate::core::save_context(
        &db.lock().unwrap(),
        ProjectContext {
            name: "Demo".into(),
            root_path: root.to_string_lossy().into(),
            instructions: "Responda em português.".into(),
            allowed_tools: tools.iter().map(|s| s.to_string()).collect(),
            references: vec!["README.md".into()],
            ai_enabled: true,
            ..Default::default()
        },
        chrono::Utc::now(),
    )
    .unwrap();
    let runner = Arc::new(ProcessRunner::default());
    let state = dir.path().join("fake-state");
    runner.env.lock().unwrap().push(("NYX_FAKE_DIR".into(), state.to_string_lossy().into()));
    let sink = Arc::new(RecSink::default());
    let ai = Arc::new(FakeAi::default());
    let engine = Engine::new(db.clone(), runner.clone(), ai.clone(), sink.clone(), Box::new(move || Some(exe.clone())));
    engine.set_worktrees_root(dir.path().join("worktrees"));
    Some(Rig { _dir: dir, root, db, runner, sink, ai, engine, ctx })
}

impl Rig {
    fn scenario(&self, s: &str) {
        let mut env = self.runner.env.lock().unwrap();
        env.retain(|(k, _)| k != "NYX_FAKE_SCENARIO" && k != "NYX_FAKE_DELAY_MS");
        env.push(("NYX_FAKE_SCENARIO".into(), s.into()));
        if s == "slow" {
            env.push(("NYX_FAKE_DELAY_MS".into(), "500".into()));
        }
    }

    fn wait(&self, id: &str) -> Delegation {
        let t0 = Instant::now();
        loop {
            let d = self.engine.get(id).unwrap();
            if !matches!(d.status, DelegationStatus::Running | DelegationStatus::Handoff) {
                return d;
            }
            assert!(t0.elapsed() < Duration::from_secs(30), "timeout esperando {id}: {:?}", d.status);
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn draft(&self, steps: &[&str]) -> DelegationDraft {
        DelegationDraft {
            context_id: self.ctx.id.clone(),
            title: "Demo".into(),
            steps: steps.iter().map(|s| s.to_string()).collect(),
            use_ai: true,
            model: None,
        }
    }
}

fn report(d: &Delegation, i: usize) -> String {
    let r = d.steps[i].report.clone().unwrap_or_default();
    format!("{}\n{}", r.title, r.lines.join("\n"))
}

#[test]
fn three_steps_adjust_handoff_block_commit_merge() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Criar o arquivo", "Refatorar", "Publicar"])).unwrap();
    assert_eq!(d.status, DelegationStatus::Draft);
    assert_eq!(d.branch, format!("nyx/deleg-{}", d.id));

    // Etapa 1
    rig.engine.start(&d.id).unwrap();
    let d1 = rig.wait(&d.id);
    assert_eq!(d1.status, DelegationStatus::Checkpoint, "{:?}", rig.sink.log.lock().unwrap());
    assert_eq!(d1.base_branch, "main");
    assert!(Path::new(&d1.worktree_path).join("fake-output.txt").is_file());
    let r = report(&d1, 0);
    assert!(r.starts_with("Etapa concluída (fake)"), "{r}");
    assert!(r.contains("ferramentas: Read,Edit"), "{r}");
    assert!(d1.steps[0].ai_summary.as_deref().unwrap().starts_with("Resumo:"));
    assert!(d1.steps[0].cost_usd > 0.0);
    assert!(rig.sink.changes.lock().unwrap().iter().any(|x| x.steps[0].live_activity.as_deref() == Some("Editando fake-output.txt")));
    let sid1 = d1.claude_session_id.clone().unwrap();
    // .nyx/ fora do git status do usuário
    assert_eq!(git(&rig.root, &["status", "--porcelain"]).unwrap(), "");

    // 1 ativa por contexto
    let other = rig.engine.create(rig.draft(&["x"])).unwrap();
    assert!(rig.engine.start(&other.id).unwrap_err().to_string().contains("ativa"));

    // Ajuste: mesmo passo, mesma sessão
    rig.engine.adjust(&d.id, "Inclua um teste").unwrap();
    let d1b = rig.wait(&d.id);
    assert_eq!(d1b.status, DelegationStatus::Checkpoint);
    assert_eq!(d1b.steps[0].attempts, 2);
    assert_eq!(d1b.steps[0].feedback, ["Inclua um teste"]);
    assert!(report(&d1b, 0).contains(&format!("resume: {sid1}")));

    // Aprovar -> commit e etapa 2 (contexto alto -> passagem)
    rig.scenario("handoff");
    rig.engine.approve(&d.id).unwrap();
    let d2 = rig.wait(&d.id);
    assert_eq!(d2.steps[0].status, StepStatus::Approved);
    assert!(d2.steps[0].commit.is_some());
    assert_eq!(d2.status, DelegationStatus::Checkpoint, "{:?}", rig.sink.log.lock().unwrap());
    assert_eq!(d2.current_step, 1);
    assert_eq!(d2.handoffs, 1);
    assert!(d2.steps[1].context_fill.unwrap() >= 0.6);
    assert!(Path::new(&d2.worktree_path).join("PASSAGEM.md").is_file());
    assert!(rig.sink.changes.lock().unwrap().iter().any(|x| x.status == DelegationStatus::Handoff));

    // Etapa 3: ferramenta fora da lista -> bloqueio (e sessão nova pela passagem)
    rig.scenario("blocked");
    rig.engine.approve(&d.id).unwrap();
    let d3 = rig.wait(&d.id);
    assert_eq!(d3.status, DelegationStatus::Blocked);
    assert!(d3.blocked_reason.as_deref().unwrap().contains("Bash"), "{:?}", d3.blocked_reason);
    assert_ne!(d3.claude_session_id.as_deref(), Some(sid1.as_str()), "a passagem abre sessão nova");
    assert!(rig.sink.log.lock().unwrap().iter().any(|l| l.starts_with("blocked")));

    // Usuário adiciona Bash ao contexto e refaz
    let mut ctx = rig.ctx.clone();
    ctx.allowed_tools.push("Bash(npm run deploy)".into());
    crate::core::save_context(&rig.db.lock().unwrap(), ctx, chrono::Utc::now()).unwrap();
    rig.scenario("success");
    rig.engine.start(&d.id).unwrap();
    let d3b = rig.wait(&d.id);
    assert_eq!(d3b.status, DelegationStatus::Checkpoint);
    assert!(report(&d3b, 2).contains("Bash(npm run deploy)"));

    rig.engine.approve(&d.id).unwrap();
    let done = rig.engine.get(&d.id).unwrap();
    assert_eq!(done.status, DelegationStatus::Done);
    assert!(done.steps.iter().all(|s| s.status == StepStatus::Approved && s.commit.is_some()));
    assert!(rig.sink.log.lock().unwrap().iter().any(|l| l == "done Demo"));
    let log = git(Path::new(&done.worktree_path), &["log", "--format=%s", "main..HEAD"]).unwrap();
    assert_eq!(log.lines().count(), 3, "{log}");
    assert!(log.lines().all(|l| l.starts_with("nyx: step ")));

    // Finalizar com merge
    let fin = rig.engine.finish(&d.id, true).unwrap();
    assert_eq!(fin.status, DelegationStatus::Done);
    assert!(rig.root.join("fake-output.txt").is_file());
    assert!(!rig.root.join("PASSAGEM.md").exists(), "a passagem não entra no commit");
    let top = git(&rig.root, &["log", "-1", "--format=%s %p"]).unwrap();
    assert!(top.starts_with("Merge Nyx delegation: Demo"), "{top}");
    assert_eq!(top.split_whitespace().filter(|w| w.len() >= 7).count() >= 2, true, "merge --no-ff tem 2 pais");
    assert!(!Path::new(&fin.worktree_path).exists(), "worktree removido");
    assert!(git(&rig.root, &["branch", "--list", &fin.branch]).unwrap().contains(&fin.branch));
    assert!(rig.engine.list(Some(&rig.ctx.id), false).unwrap().iter().all(|x| x.id != d.id));
    assert!(rig.engine.list(Some(&rig.ctx.id), true).unwrap().iter().any(|x| x.id == d.id));
    assert!(rig.ai.calls.lock().unwrap().iter().filter(|c| c.starts_with("summary:")).count() >= 4);
    assert!(rig.db.lock().unwrap().ai_cost_since(chrono::Utc::now() - chrono::Duration::days(1)).unwrap() > 0.0);
}

#[test]
fn error_then_cancel_kills_the_process() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("error");
    let d = rig.engine.create(rig.draft(&["Quebrar"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    let f = rig.wait(&d.id);
    assert_eq!(f.status, DelegationStatus::Failed);
    assert_eq!(f.steps[0].status, StepStatus::Failed);
    assert!(rig.sink.log.lock().unwrap().iter().any(|l| l.contains("error_max_turns")));

    // Refaz em modo lento e cancela no meio
    rig.scenario("slow");
    rig.engine.start(&d.id).unwrap();
    std::thread::sleep(Duration::from_millis(700));
    let t0 = Instant::now();
    let c = rig.engine.cancel(&d.id).unwrap();
    assert_eq!(c.status, DelegationStatus::Cancelled);
    assert_eq!(c.steps[0].status, StepStatus::Failed);
    std::thread::sleep(Duration::from_millis(1500));
    let after = rig.engine.get(&d.id).unwrap();
    assert_eq!(after.status, DelegationStatus::Cancelled, "a thread não sobrescreve o cancelamento");
    assert!(t0.elapsed() < Duration::from_secs(5));
    assert!(Path::new(&after.worktree_path).is_dir(), "worktree mantido");
    assert!(rig.engine.finish(&d.id, true).is_err(), "cancelada não faz merge");
    rig.engine.finish(&d.id, false).unwrap();
}

#[test]
fn recover_marks_running_as_failed() {
    let Some(rig) = rig(&["Read"]) else { return };
    rig.scenario("slow");
    let d = rig.engine.create(rig.draft(&["Demorar"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    // Simula a Nyx fechando: um motor novo sobre o mesmo banco
    let fresh = Engine::new(rig.db.clone(), rig.runner.clone(), rig.ai.clone(), rig.sink.clone(), Box::new(|| None));
    assert_eq!(fresh.recover().unwrap(), 1);
    let r = fresh.get(&d.id).unwrap();
    assert_eq!(r.status, DelegationStatus::Failed);
    assert_eq!(r.steps[0].report.as_ref().unwrap().title, "Interrompida");
    rig.engine.cancel(&d.id).unwrap();
}

#[test]
fn create_requires_git_and_tools() {
    let Some(rig) = rig(&[]) else { return };
    let e = rig.engine.create(rig.draft(&["x"])).unwrap_err().to_string();
    assert!(e.contains("ferramentas permitidas"), "{e}");
    let nogit = tempfile::tempdir().unwrap();
    let ctx = crate::core::save_context(
        &rig.db.lock().unwrap(),
        ProjectContext {
            name: "Sem git".into(),
            root_path: nogit.path().to_string_lossy().into(),
            allowed_tools: vec!["Read".into()],
            ..Default::default()
        },
        chrono::Utc::now(),
    )
    .unwrap();
    let mut dr = rig.draft(&["x"]);
    dr.context_id = ctx.id;
    assert!(rig.engine.create(dr).unwrap_err().to_string().contains("git"));
}

/// Alvo das ações do chat sobre o motor (o mesmo caminho dos commands).
struct EngineTarget(Arc<Engine>);

impl ActionTarget for EngineTarget {
    fn start_delegation(&self, d: DelegationDraft) -> anyhow::Result<()> {
        let x = self.0.create(d)?;
        self.0.start(&x.id).map(|_| ())
    }
    fn approve_step(&self, id: &str) -> anyhow::Result<()> {
        self.0.approve(id).map(|_| ())
    }
    fn adjust_step(&self, id: &str, f: &str) -> anyhow::Result<()> {
        self.0.adjust(id, f).map(|_| ())
    }
    fn install_hooks(&self, _: bool) -> anyhow::Result<()> {
        anyhow::bail!("não nos testes")
    }
    fn create_context(&self, _: ProjectContext) -> anyhow::Result<()> {
        Ok(())
    }
    fn open_panel(&self, _: Option<&str>) -> anyhow::Result<()> {
        Ok(())
    }
    fn delegation(&self, id: &str) -> anyhow::Result<Delegation> {
        self.0.get(id)
    }
    fn context_exists(&self, _: &str) -> bool {
        true
    }
}

#[test]
fn chat_proposes_and_user_confirms_approve() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Um", "Dois"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    assert_eq!(rig.wait(&d.id).status, DelegationStatus::Checkpoint);

    rig.ai.replies.lock().unwrap().push(ChatReply {
        text: "A etapa 1 terminou limpa. Aprovo?".into(),
        actions: vec![ProposedAction {
            id: "toolu_9".into(),
            kind: "approveStep".into(),
            label: "Aprovar etapa 1".into(),
            args: serde_json::json!({ "delegationId": d.id }).as_object().unwrap().clone(),
        }],
        cost_usd: 0.002,
    });
    let snap = "snapshot compacto";
    let (_, reply) = chat::send(&rig.db, rig.ai.as_ref(), Some(&rig.ctx.id), "e a delegação?", snap, chrono::Utc::now()).unwrap();
    assert_eq!(reply.actions.len(), 1);
    // Nada rodou antes do clique
    assert_eq!(rig.engine.get(&d.id).unwrap().current_step, 0);

    let target = EngineTarget(rig.engine.clone());
    let m = chat::run_action(&rig.db, reply.id, "toolu_9", &target).unwrap();
    assert_eq!(m.actions[0].state, ChatActionState::Done);
    let next = rig.wait(&d.id);
    assert_eq!(next.current_step, 1);
    assert!(next.steps[0].commit.is_some());
    // Uma vez só
    assert!(chat::run_action(&rig.db, reply.id, "toolu_9", &target).is_err());
}

/// Chat com o cliente real do `nyx-ai` contra um servidor HTTP falso (nada sai da máquina):
/// pergunta -> resposta com 1 ação proposta -> confirmar -> o command roda.
#[test]
fn chat_with_real_client_and_fake_api_server() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Um", "Dois"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    assert_eq!(rig.wait(&d.id).status, DelegationStatus::Checkpoint);

    let body = serde_json::json!({
        "content": [
            {"type": "text", "text": "Etapa 1 pronta, Dev. Quer aprovar?"},
            {"type": "tool_use", "id": "toolu_x", "name": "propose_approve_step",
             "input": {"delegationId": d.id, "label": "Aprovar etapa 1"}}
        ],
        "stop_reason": "tool_use",
        "model": "claude-haiku-5-5",
        "usage": {"input_tokens": 900, "output_tokens": 60}
    });
    let seen = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let seen2 = seen.clone();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new().route(
        "/v1/messages",
        axum::routing::post(move |req: axum::Json<serde_json::Value>| {
            let body = body.clone();
            let seen = seen2.clone();
            async move {
                seen.lock().unwrap().push(req.0);
                axum::Json(body)
            }
        }),
    );
    rt.spawn(async move { axum::serve(listener, app).await.unwrap() });

    let ai = crate::ai::NyxAi::with_client(nyx_ai::Client::new("test-key", format!("http://127.0.0.1:{port}")));
    let (_, reply) =
        chat::send(&rig.db, &ai, Some(&rig.ctx.id), "e a delegação?", "snapshot local", chrono::Utc::now()).unwrap();
    assert_eq!(reply.text, "Etapa 1 pronta, Dev. Quer aprovar?");
    assert_eq!(reply.actions.len(), 1);
    assert_eq!(reply.actions[0].kind, ChatActionKind::ApproveStep);
    assert!(reply.cost_usd > 0.0);
    let req = seen.lock().unwrap()[0].clone();
    assert!(req["system"].as_str().unwrap_or("").contains("snapshot local") || req.to_string().contains("snapshot local"));
    assert!(req["tools"].as_array().map_or(false, |t| !t.is_empty()));

    let m = chat::run_action(&rig.db, reply.id, "toolu_x", &EngineTarget(rig.engine.clone())).unwrap();
    assert_eq!(m.actions[0].state, ChatActionState::Done);
    assert_eq!(rig.wait(&d.id).current_step, 1);
}

#[test]
fn hardening_limits_paths_and_no_shell() {
    assert!(crate::runner::check_not_shell_script(Path::new("C:/x/claude.cmd")).is_err());
    assert!(crate::runner::check_not_shell_script(Path::new("C:/x/claude.BAT")).is_err());
    assert!(crate::runner::check_not_shell_script(Path::new("C:/x/claude.exe")).is_ok());

    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    let long = "x".repeat(crate::delegation::MAX_STEP_CHARS + 1);
    assert!(rig.engine.create(rig.draft(&[long.as_str()])).unwrap_err().to_string().contains("caracteres"));
    let many: Vec<&str> = vec!["a"; crate::delegation::MAX_STEPS + 1];
    assert!(rig.engine.create(rig.draft(&many)).is_err());

    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Um"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    let d = rig.wait(&d.id);
    assert_eq!(d.status, DelegationStatus::Checkpoint);
    // exclude no repo do contexto, não no .gitignore versionado
    // worktree fora da árvore do projeto: <worktrees>\<context-id>\<deleg-id>
    let wt = PathBuf::from(&d.worktree_path);
    assert!(wt.ends_with(Path::new(&rig.ctx.id).join(&d.id)), "{}", wt.display());
    assert!(!wt.starts_with(&rig.root));
    assert!(!rig.root.join(".nyx").exists());
    assert!(!rig.root.join(".gitignore").exists());
    let fb = "y".repeat(crate::delegation::MAX_FEEDBACK_CHARS + 1);
    assert!(rig.engine.adjust(&d.id, &fb).is_err());

    // worktree adulterado no banco (fora de <root>\.nyx\worktrees) -> recusado
    let outside = tempfile::tempdir().unwrap();
    rig.db
        .lock()
        .unwrap()
        .conn()
        .execute("UPDATE delegations SET worktree_path = ?1 WHERE id = ?2", rusqlite::params![outside.path().to_string_lossy(), d.id])
        .unwrap();
    let f = rig.engine.approve(&d.id).unwrap();
    assert_eq!(f.status, DelegationStatus::Failed);
    assert!(f.blocked_reason.as_deref().unwrap_or("").contains("fora de"), "{:?}", f.blocked_reason);
}

#[cfg(windows)]
#[test]
fn child_never_sees_the_api_key() {
    use crate::runner::{AgentRunner, RunControl};
    let r = ProcessRunner::default();
    r.env.lock().unwrap().push(("ANTHROPIC_API_KEY".into(), "sk-segredo".into()));
    r.env.lock().unwrap().push(("NYX_TEST_VISIBLE".into(), "ok".into()));
    let cmd = nyx_claude::delegate::StepCommand {
        program: PathBuf::from("cmd.exe"),
        args: vec!["/c".into(), "echo".into(), "[%ANTHROPIC_API_KEY%][%NYX_TEST_VISIBLE%]".into()],
        cwd: std::env::temp_dir(),
        stdin: "prompt pelo stdin".into(),
    };
    let lines = r.run(&cmd, &RunControl::default(), &mut |_| {}).unwrap();
    let out = lines.join("\n");
    assert!(!out.contains("sk-segredo"), "{out}");
    assert!(out.contains("[%ANTHROPIC_API_KEY%][ok]"), "{out}");
}

#[test]
fn double_approve_runs_once() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Um", "Dois"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    assert_eq!(rig.wait(&d.id).status, DelegationStatus::Checkpoint);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let hs: Vec<_> = (0..2)
        .map(|_| {
            let (e, b, id) = (rig.engine.clone(), barrier.clone(), d.id.clone());
            std::thread::spawn(move || {
                b.wait();
                e.approve(&id).is_ok()
            })
        })
        .collect();
    let ok: usize = hs.into_iter().map(|h| h.join().unwrap() as usize).sum();
    assert_eq!(ok, 1, "só um clique vale");
    let after = rig.wait(&d.id);
    assert_eq!(after.current_step, 1);
    assert_eq!(after.steps[1].attempts, 1, "um begin_step só");
    let log = git(Path::new(&after.worktree_path), &["log", "--format=%s", "main..HEAD"]).unwrap();
    assert_eq!(log.lines().count(), 1);
    let d2 = rig.engine.create(rig.draft(&["x"])).unwrap();
    assert!(rig.engine.start(&d2.id).is_err(), "contexto já tem uma ativa");
}

#[test]
fn merge_refuses_when_user_merge_in_progress() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Um"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    rig.wait(&d.id);
    rig.engine.approve(&d.id).unwrap();
    let head = git(&rig.root, &["rev-parse", "HEAD"]).unwrap();
    let merge_head = rig.root.join(".git").join("MERGE_HEAD");
    std::fs::write(&merge_head, format!("{head}\n")).unwrap();
    let e = rig.engine.finish(&d.id, true).unwrap_err().to_string();
    assert!(e.contains("merge em andamento"), "{e}");
    assert!(merge_head.exists(), "o merge do usuário fica intacto");
    std::fs::remove_file(&merge_head).unwrap();
    std::fs::write(rig.root.join("README.md"), "mudou\n").unwrap();
    assert!(rig.engine.finish(&d.id, true).unwrap_err().to_string().contains("não commitadas"));
    git(&rig.root, &["checkout", "--", "README.md"]).unwrap();
    rig.engine.finish(&d.id, true).unwrap();
}

#[test]
fn kill_all_stops_running_agents() {
    let Some(rig) = rig(&["Read"]) else { return };
    rig.scenario("slow");
    let d = rig.engine.create(rig.draft(&["Demorar"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(rig.engine.kill_all(), 1);
    std::thread::sleep(Duration::from_millis(1500));
    let r = rig.engine.get(&d.id).unwrap();
    assert_ne!(r.status, DelegationStatus::Checkpoint);
    assert_eq!(rig.engine.kill_all(), 0);
}

/// Alvo que conhece só um contexto (valida o contextId).
struct StrictTarget(Arc<Engine>, String, Mutex<Vec<ProjectContext>>);

impl ActionTarget for StrictTarget {
    fn start_delegation(&self, d: DelegationDraft) -> anyhow::Result<()> {
        assert!(!d.use_ai && d.model.is_none(), "modelo/IA nunca vêm do modelo");
        Ok(())
    }
    fn approve_step(&self, id: &str) -> anyhow::Result<()> {
        self.0.approve(id).map(|_| ())
    }
    fn adjust_step(&self, id: &str, f: &str) -> anyhow::Result<()> {
        self.0.adjust(id, f).map(|_| ())
    }
    fn install_hooks(&self, _: bool) -> anyhow::Result<()> {
        Ok(())
    }
    fn create_context(&self, c: ProjectContext) -> anyhow::Result<()> {
        self.2.lock().unwrap().push(c);
        Ok(())
    }
    fn open_panel(&self, _: Option<&str>) -> anyhow::Result<()> {
        Ok(())
    }
    fn delegation(&self, id: &str) -> anyhow::Result<Delegation> {
        self.0.get(id)
    }
    fn context_exists(&self, id: &str) -> bool {
        id == self.1
    }
}

#[test]
fn chat_action_args_are_validated() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Um", "Dois"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    rig.wait(&d.id);
    let t = StrictTarget(rig.engine.clone(), rig.ctx.id.clone(), Mutex::new(vec![]));
    let mk = |kind: &str, args: serde_json::Value| -> ChatAction {
        serde_json::from_value(serde_json::json!({"id": "a1", "kind": kind, "label": "x", "args": args, "state": "proposed"}))
            .unwrap()
    };
    let post = |ctx: Option<&str>, a: ChatAction| {
        let m = chat::insert(rig.db.lock().unwrap().conn(), ctx, ChatRole::Nyx, "?", &[a], 0.0, chrono::Utc::now()).unwrap();
        chat::run_action(&rig.db, m.id, "a1", &t)
    };
    let e = post(Some("outro"), mk("approveStep", serde_json::json!({"delegationId": d.id}))).unwrap_err().to_string();
    assert!(e.contains("outro contexto"), "{e}");
    assert!(post(None, mk("startDelegation", serde_json::json!({"contextId": "nao-existe", "title": "t", "steps": ["a"]}))).is_err());
    post(None, mk("startDelegation", serde_json::json!({"contextId": rig.ctx.id, "title": "t", "steps": ["a"], "useAi": true, "model": "--x"})))
        .unwrap();
    assert!(post(None, mk("createContext", serde_json::json!({"name": "n", "rootPath": "relativo"}))).is_err());
    let home = dirs::home_dir().unwrap();
    assert!(post(None, mk("createContext", serde_json::json!({"name": "n", "rootPath": home}))).is_err());
    post(None, mk("createContext", serde_json::json!({"name": "n", "rootPath": rig.root, "allowedTools": ["Bash"], "aiEnabled": true})))
        .unwrap();
    let c = t.2.lock().unwrap()[0].clone();
    assert!(c.allowed_tools.is_empty() && !c.ai_enabled && c.instructions.is_empty());
    post(Some(&rig.ctx.id), mk("approveStep", serde_json::json!({"delegationId": d.id}))).unwrap();
    assert!(post(Some(&rig.ctx.id), mk("approveStep", serde_json::json!({"delegationId": d.id}))).is_err());
    rig.wait(&d.id);
}

/// Fechar o job (o que o SO faz quando a Nyx sai ou cai) mata o `claude` que está nele.
#[cfg(windows)]
#[test]
fn job_object_kills_agent_when_closed() {
    let Some(exe) = fake_exe() else { return };
    let mut child = std::process::Command::new(exe)
        .args(["-p", "--output-format", "stream-json"])
        .env("NYX_FAKE_SCENARIO", "slow")
        .env("NYX_FAKE_DELAY_MS", "2000")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let job = crate::job::Job::new().expect("job object");
    assert!(job.assign(&child));
    std::thread::sleep(Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none(), "ainda rodando antes de fechar o job");
    drop(job);
    let t0 = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(5), "o processo sobreviveu ao fechamento do job");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn approve_anyway_from_blocked() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("blocked");
    let d = rig.engine.create(rig.draft(&["Publicar", "Depois"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    assert_eq!(rig.wait(&d.id).status, DelegationStatus::Blocked);
    rig.scenario("success");
    rig.engine.approve(&d.id).unwrap();
    let n = rig.wait(&d.id);
    assert_eq!(n.current_step, 1, "seguiu para a próxima etapa");
    assert_eq!(n.steps[0].status, StepStatus::Approved);
    assert_eq!(n.status, DelegationStatus::Checkpoint);
    assert!(n.blocked_reason.is_none());
}

#[test]
fn precommit_failure_becomes_failed_and_retry_commits_only() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Um", "Dois"])).unwrap();
    rig.engine.start(&d.id).unwrap();
    rig.wait(&d.id);
    let hook = rig.root.join(".git").join("hooks").join("pre-commit");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/bin/sh\necho 'lint falhou: fake-output.txt:1'\necho 'linha 2'\nexit 1\n").unwrap();
    let f = rig.engine.approve(&d.id).unwrap();
    assert_eq!(f.status, DelegationStatus::Failed);
    let reason = f.blocked_reason.clone().unwrap();
    let mut lines = reason.lines();
    assert!(lines.next().unwrap().contains("pre-commit"), "{reason}");
    assert!(reason.contains("lint falhou"), "{reason}");
    assert!(reason.lines().count() <= 6);
    assert!(f.steps[0].commit.is_none());
    let attempts = f.steps[0].attempts;
    // corrige e aprova de novo: só o commit, sem rodar a etapa outra vez
    std::fs::remove_file(&hook).unwrap();
    let ok = rig.engine.approve(&d.id).unwrap();
    assert!(ok.steps[0].commit.is_some());
    assert_eq!(ok.steps[0].attempts, attempts);
    assert!(ok.blocked_reason.is_none());
    rig.wait(&d.id);
}

#[test]
fn repo_without_commits_is_refused() {
    let Some(rig) = rig(&["Read"]) else { return };
    let empty = tempfile::tempdir().unwrap();
    git(empty.path(), &["init", "-q"]).unwrap();
    let ctx = crate::core::save_context(
        &rig.db.lock().unwrap(),
        ProjectContext {
            name: "Vazio".into(),
            root_path: empty.path().to_string_lossy().into(),
            allowed_tools: vec!["Read".into()],
            ..Default::default()
        },
        chrono::Utc::now(),
    )
    .unwrap();
    let mut dr = rig.draft(&["x"]);
    dr.context_id = ctx.id;
    let e = rig.engine.create(dr).unwrap_err().to_string();
    assert!(e.contains("nenhum commit"), "{e}");
}

#[test]
fn orphan_branch_is_cleaned_and_updated_at_is_monotonic() {
    let Some(rig) = rig(&["Read", "Edit"]) else { return };
    rig.scenario("success");
    let d = rig.engine.create(rig.draft(&["Um"])).unwrap();
    // sobra de uma tentativa anterior que falhou depois do worktree add
    git(&rig.root, &["branch", &d.branch]).unwrap();
    rig.engine.start(&d.id).unwrap();
    assert_eq!(rig.wait(&d.id).status, DelegationStatus::Checkpoint);
    let ups: Vec<String> = rig.sink.changes.lock().unwrap().iter().filter(|x| x.id == d.id).map(|x| x.updated_at.clone()).collect();
    assert!(ups.windows(2).all(|w| w[0] <= w[1]), "{ups:?}");
    let mut distinct = ups.clone();
    distinct.dedup();
    assert!(distinct.len() >= 3);
}
