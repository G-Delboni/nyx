//! Regras puras de sessão: status por tipo de evento, contexto por prefixo de `cwd`.

use crate::model::{EventKind, Priority, ProjectContext, SessionStatus};

/// Sessão sem evento por este tempo vira `idle`.
pub const IDLE_AFTER_MINUTES: i64 = 30;

/// Estados que viram `idle` por inatividade. `waiting` e `error` ficam como estão
/// de propósito: são sinais de que o usuário precisa agir e não devem sumir sozinhos.
pub const IDLE_FROM: &[SessionStatus] = &[SessionStatus::Running, SessionStatus::Done];

/// Normaliza um caminho para comparação: `\` -> `/`, minúsculas, sem `/` no fim.
pub fn normalize_path(p: &str) -> String {
    let mut s = p.trim().replace('\\', "/").to_lowercase();
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

/// Contexto cujo `root_path` é prefixo do `cwd` (respeitando a fronteira de pasta).
/// Com vários candidatos, vence o mais específico (raiz mais longa).
pub fn context_for(cwd: Option<&str>, contexts: &[ProjectContext]) -> Option<String> {
    let cwd = normalize_path(cwd?);
    contexts
        .iter()
        .filter_map(|c| {
            let root = normalize_path(&c.root_path);
            if root.is_empty() {
                return None;
            }
            let hit = cwd == root || cwd.starts_with(&format!("{root}/"));
            hit.then_some((root.len(), c.id.clone()))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, id)| id)
}

/// Último segmento do `cwd` (nome da pasta), como o usuário vê.
pub fn folder_name(cwd: Option<&str>) -> Option<String> {
    let cwd = cwd?.trim().trim_end_matches(['/', '\\']);
    cwd.rsplit(['/', '\\']).find(|s| !s.is_empty()).map(str::to_string)
}

pub fn status_for(kind: EventKind) -> Option<SessionStatus> {
    Some(match kind {
        EventKind::SessionStart | EventKind::PromptSubmitted | EventKind::ToolUse => SessionStatus::Running,
        EventKind::NeedsInput => SessionStatus::Waiting,
        EventKind::TaskDone => SessionStatus::Done,
        EventKind::SessionEnd => SessionStatus::Ended,
        EventKind::Error => SessionStatus::Error,
        EventKind::UsageThreshold => return None,
    })
}

pub fn priority_for(kind: EventKind) -> Priority {
    match kind {
        EventKind::TaskDone | EventKind::NeedsInput | EventKind::Error => Priority::High,
        EventKind::UsageThreshold => Priority::Medium,
        _ => Priority::Low,
    }
}

/// `PreToolUse` e `UserPromptSubmit` chegam a cada passo do agente: só mudam o status,
/// não viram linha em `events` (senão a lista de eventos vira ruído).
pub fn should_store(kind: EventKind) -> bool {
    !matches!(kind, EventKind::ToolUse | EventKind::PromptSubmitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(id: &str, root: &str) -> ProjectContext {
        ProjectContext {
            id: id.into(),
            name: id.into(),
            root_path: root.into(),
            instructions: String::new(),
            color: None,
            created_at: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize_path("C:\\Users\\Dev\\Docs\\"), "c:/users/dev/docs");
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn context_by_prefix() {
        let cs = vec![
            ctx("docs", "C:/Users/dev/Documents"),
            ctx("cd", "c:\\users\\dev\\documents\\projeto demo\\"),
        ];
        let f = |p| context_for(Some(p), &cs);
        assert_eq!(f("C:\\Users\\dev\\Documents\\projeto demo\\ep1").as_deref(), Some("cd"));
        assert_eq!(f("C:\\Users\\dev\\Documents\\Projeto Demo").as_deref(), Some("cd"));
        assert_eq!(f("C:\\Users\\dev\\Documents\\nyx").as_deref(), Some("docs"));
        // fronteira de pasta: "Documents2" não está dentro de "Documents"
        assert_eq!(f("C:\\Users\\dev\\Documents2"), None);
        assert_eq!(context_for(None, &cs), None);
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_name(Some("C:\\Users\\dev\\Documents\\nyx\\")).as_deref(), Some("nyx"));
        assert_eq!(folder_name(Some("/home/x/projeto demo")).as_deref(), Some("projeto demo"));
        assert_eq!(folder_name(None), None);
    }

    #[test]
    fn status_map() {
        assert_eq!(status_for(EventKind::ToolUse), Some(SessionStatus::Running));
        assert_eq!(status_for(EventKind::NeedsInput), Some(SessionStatus::Waiting));
        assert_eq!(status_for(EventKind::TaskDone), Some(SessionStatus::Done));
        assert_eq!(status_for(EventKind::SessionEnd), Some(SessionStatus::Ended));
        assert!(!should_store(EventKind::ToolUse));
        assert!(should_store(EventKind::TaskDone));
    }
}
