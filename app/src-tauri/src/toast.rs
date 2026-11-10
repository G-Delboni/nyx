//! Textos dos toasts (tom da `docs/PERSONAGEM.md` §2: seco, sem exclamação, sem emoji)
//! e a regra do "não perturbe".

use crate::model::{EventKind, Priority, Settings, SpeechKind};
use chrono::NaiveTime;

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub title: String,
    pub body: String,
    pub priority: Priority,
    /// Evento a que o toast se refere (para os botões de ação).
    pub event_id: Option<i64>,
    /// O que a Nyx fala junto com o toast (frase curta, nunca a saída crua). `None` = calada.
    pub speech: Option<Speech>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Speech {
    pub kind: SpeechKind,
    pub text: String,
}

pub fn speech(kind: SpeechKind, text: impl Into<String>) -> Option<Speech> {
    Some(Speech { kind, text: text.into() })
}

fn folder_or_default(folder: Option<&str>) -> &str {
    folder.filter(|f| !f.is_empty()).unwrap_or("um projeto")
}

/// Toast de um evento de hook. `None` = não avisa (eventos de baixa prioridade).
pub fn for_event(kind: EventKind, folder: Option<&str>, detail: Option<&str>) -> Option<Toast> {
    let f = folder_or_default(folder);
    let (title, body, said) = match kind {
        // Conclusão comum só avisa por toast; a longa fala (`for_long_task`).
        EventKind::TaskDone => ("Claude Code concluiu".to_string(), f.to_string(), None),
        EventKind::NeedsInput => {
            let body = format!("Dev, o Claude Code em {f} quer uma resposta sua.");
            ("Aguardando você".to_string(), body.clone(), speech(SpeechKind::Alert, body))
        }
        EventKind::Error => {
            let line = format!("Dev, o Claude Code em {f} falhou.");
            let mut body = line.clone();
            if let Some(d) = detail.filter(|d| !d.is_empty()) {
                body.push(' ');
                body.push_str(&first_line(d, 120));
            }
            ("Erro".to_string(), body, speech(SpeechKind::Alert, line))
        }
        _ => return None,
    };
    Some(Toast { title, body, priority: crate::sessions::priority_for(kind), event_id: None, speech: said })
}

/// `Stop` depois de uma tarefa longa: prioridade média (o "não perturbe" segura).
pub fn for_long_task(folder: Option<&str>, minutes: i64) -> Toast {
    let f = folder_or_default(folder);
    Toast {
        title: "Claude Code concluiu".to_string(),
        body: format!("Tarefa longa finalizada em {f}. {minutes} min."),
        priority: Priority::Medium,
        event_id: None,
        speech: speech(SpeechKind::LongTask, format!("Tarefa longa finalizada em {f}. {minutes} minutos.")),
    }
}

/// Toast de uso semanal perto do teto. Sem humor em alerta de uso (PERSONAGEM §2).
pub fn for_usage(threshold: f64, percent: f64, cost: f64, ceiling: f64) -> Toast {
    let who = if threshold >= 95.0 { "Dev, semana" } else { "Semana" };
    Toast {
        title: format!("Uso em {}%", threshold.round() as i64),
        body: format!(
            "{who} em {}% do teto estimado. US$ {:.2} de US$ {:.2}.",
            percent.floor() as i64,
            cost,
            ceiling
        ),
        priority: Priority::Medium,
        event_id: None,
        speech: speech(SpeechKind::LongTask, format!("{who} em {}% do teto.", percent.floor() as i64)),
    }
}

fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        line.to_string()
    } else {
        let cut: String = line.chars().take(max).collect();
        format!("{cut}...")
    }
}

fn parse_hm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

/// "Não perturbe" ativo agora? Toggle manual ou dentro da janela de horário
/// (que pode cruzar a meia-noite, ex. 22:00..08:00).
pub fn dnd_active(s: &Settings, now: NaiveTime) -> bool {
    if s.do_not_disturb {
        return true;
    }
    let Some(sched) = &s.dnd_schedule else { return false };
    let (Some(start), Some(end)) = (parse_hm(&sched.start), parse_hm(&sched.end)) else {
        return false;
    };
    if start <= end {
        now >= start && now < end
    } else {
        now >= start || now < end
    }
}

/// O "não perturbe" segura só os de prioridade média (e baixa). Alta sempre passa.
pub fn should_show(t: &Toast, dnd: bool) -> bool {
    match t.priority {
        Priority::High => true,
        Priority::Medium => !dnd,
        Priority::Low => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DndSchedule;

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn texts_have_no_exclamation_or_emoji() {
        let all = [
            for_event(EventKind::TaskDone, Some("nyx"), None).unwrap(),
            for_event(EventKind::NeedsInput, Some("projeto demo"), None).unwrap(),
            for_event(EventKind::Error, None, Some("build quebrou\nlinha 2")).unwrap(),
            for_usage(80.0, 82.4, 41.2, 50.0),
            for_usage(95.0, 96.0, 48.0, 50.0),
            for_long_task(Some("nyx"), 14),
        ];
        for t in &all {
            let s = format!("{} {}", t.title, t.body);
            assert!(!s.contains('!'), "{s}");
            assert!(s.chars().all(|c| (c as u32) < 0x2000), "{s}");
        }
        assert_eq!(all[0].title, "Claude Code concluiu");
        assert_eq!(all[0].body, "nyx");
        assert_eq!(all[1].body, "Dev, o Claude Code em projeto demo quer uma resposta sua.");
        assert!(all[2].body.ends_with("build quebrou"));
        assert_eq!(all[3].body, "Semana em 82% do teto estimado. US$ 41.20 de US$ 50.00.");
        assert!(all[4].body.starts_with("Dev"));
        assert_eq!(all[5].body, "Tarefa longa finalizada em nyx. 14 min.");
        assert_eq!(all[5].priority, Priority::Medium);
        assert!(for_event(EventKind::ToolUse, None, None).is_none());
        // fala: frase curta, sem o detalhe cru do erro
        assert_eq!(all[0].speech, None);
        assert_eq!(all[1].speech.as_ref().unwrap().kind, SpeechKind::Alert);
        assert_eq!(all[2].speech.as_ref().unwrap().text, "Dev, o Claude Code em um projeto falhou.");
        assert_eq!(all[3].speech.as_ref().unwrap().text, "Semana em 82% do teto.");
        assert_eq!(all[3].speech.as_ref().unwrap().kind, SpeechKind::LongTask);
        assert_eq!(all[5].speech.as_ref().unwrap().text, "Tarefa longa finalizada em nyx. 14 minutos.");
    }

    #[test]
    fn dnd_rules() {
        let mut s = Settings::defaults(10.0);
        assert!(!dnd_active(&s, hm(23, 0)));
        s.dnd_schedule = Some(DndSchedule { start: "22:00".into(), end: "08:00".into() });
        assert!(dnd_active(&s, hm(23, 0)));
        assert!(dnd_active(&s, hm(7, 59)));
        assert!(!dnd_active(&s, hm(8, 0)));
        s.dnd_schedule = Some(DndSchedule { start: "13:00".into(), end: "14:00".into() });
        assert!(dnd_active(&s, hm(13, 30)));
        assert!(!dnd_active(&s, hm(12, 0)));
        s.dnd_schedule = None;
        s.do_not_disturb = true;
        assert!(dnd_active(&s, hm(12, 0)));

        let high = for_event(EventKind::TaskDone, None, None).unwrap();
        let med = for_usage(80.0, 80.0, 1.0, 1.0);
        assert!(should_show(&high, true));
        assert!(!should_show(&med, true));
        assert!(should_show(&med, false));
    }
}
