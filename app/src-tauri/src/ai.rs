//! IA opt-in da Nyx (Haiku) atrás de um trait: em produção, o crate `nyx-ai`; nos
//! testes, um falso. Sem `NYX_ANTHROPIC_API_KEY` no start, a IA fica indisponível e o
//! resto funciona. Nada de transcripts nem código vai para a API.

use anyhow::Result;
pub use nyx_ai::{ChatTurn, ProposedAction, Role};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatReply {
    pub text: String,
    pub actions: Vec<ProposedAction>,
    pub cost_usd: f64,
}

pub trait AiBackend: Send + Sync {
    /// Há cliente (chave presente no start).
    fn available(&self) -> bool;
    fn model(&self) -> String;
    /// Resumo (≤ 5 linhas) do relatório do agente + `git status --short` do worktree.
    fn summarize_step(&self, report: &str, diff_stat: &str) -> Result<(String, f64)>;
    /// Passos propostos para um objetivo; `context` = nome + instruções do contexto.
    fn plan_steps(&self, goal: &str, context: &str) -> Result<(Vec<String>, f64)>;
    /// `history` sem a pergunta nova, que vai em `user_text`.
    fn chat(&self, snapshot: &str, history: &[ChatTurn], user_text: &str) -> Result<ChatReply>;
}

/// Backend real (`nyx-ai`): cliente criado no start a partir do ambiente.
pub struct NyxAi {
    client: Option<nyx_ai::Client>,
}

impl NyxAi {
    pub fn from_env() -> Self {
        Self { client: nyx_ai::Client::from_env().ok() }
    }

    pub fn with_client(client: nyx_ai::Client) -> Self {
        Self { client: Some(client) }
    }

    fn client(&self) -> Result<&nyx_ai::Client> {
        self.client.as_ref().ok_or_else(|| anyhow::anyhow!("IA indisponível: defina NYX_ANTHROPIC_API_KEY e reinicie a Nyx"))
    }
}

impl AiBackend for NyxAi {
    fn available(&self) -> bool {
        self.client.is_some()
    }
    fn model(&self) -> String {
        nyx_ai::DEFAULT_MODEL.to_string()
    }
    fn summarize_step(&self, report: &str, diff_stat: &str) -> Result<(String, f64)> {
        let g = self.client()?.summarize_step(report, diff_stat)?;
        Ok((g.text, g.cost_usd))
    }
    fn plan_steps(&self, goal: &str, context: &str) -> Result<(Vec<String>, f64)> {
        let p = self.client()?.plan_steps(goal, context)?;
        Ok((p.steps, p.cost_usd))
    }
    fn chat(&self, snapshot: &str, history: &[ChatTurn], user_text: &str) -> Result<ChatReply> {
        let r = self.client()?.chat(snapshot, history, user_text)?;
        Ok(ChatReply { text: r.text, actions: r.actions, cost_usd: r.cost_usd })
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    /// IA falsa para testes: respostas programadas e registro das chamadas.
    #[derive(Default)]
    pub struct FakeAi {
        pub replies: Mutex<Vec<ChatReply>>,
        pub calls: Mutex<Vec<String>>,
        pub off: bool,
    }

    impl AiBackend for FakeAi {
        fn available(&self) -> bool {
            !self.off
        }
        fn model(&self) -> String {
            "fake-haiku".into()
        }
        fn summarize_step(&self, report: &str, diff: &str) -> Result<(String, f64)> {
            self.calls.lock().unwrap().push(format!("summary:{}|{}", report.lines().next().unwrap_or(""), diff.trim()));
            Ok((format!("Resumo: {}", report.lines().next().unwrap_or("")), 0.001))
        }
        fn plan_steps(&self, goal: &str, _: &str) -> Result<(Vec<String>, f64)> {
            self.calls.lock().unwrap().push(format!("plan:{goal}"));
            Ok((vec![format!("Entender: {goal}"), "Implementar".into(), "Testar".into()], 0.002))
        }
        fn chat(&self, snapshot: &str, history: &[ChatTurn], user_text: &str) -> Result<ChatReply> {
            if self.off {
                anyhow::bail!("IA indisponível");
            }
            self.calls.lock().unwrap().push(format!("chat:{}|{}|{user_text}", snapshot.len(), history.len()));
            let mut r = self.replies.lock().unwrap();
            Ok(if r.is_empty() { ChatReply { text: "Ok.".into(), ..Default::default() } } else { r.remove(0) })
        }
    }
}
