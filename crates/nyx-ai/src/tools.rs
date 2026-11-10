//! Tool schemas the chat offers to the model and the mapping of its tool calls to
//! proposed actions (`ChatAction.kind` in app/src/api/contract.ts). The model only
//! proposes; the core runs an action after the user's click.

use crate::ContentBlock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// A proposal from the model. `kind` matches `ChatAction.kind` in the UI contract
/// (`startDelegation | approveStep | adjustStep | installHooks | openPanel | createContext`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedAction {
    /// The `tool_use` id from the API.
    pub id: String,
    pub kind: String,
    /// Button text, pt-BR.
    pub label: String,
    /// Tool input without `label`.
    pub args: Map<String, Value>,
}

struct Spec {
    tool: &'static str,
    kind: &'static str,
    default_label: &'static str,
    description: &'static str,
    /// (name, schema, required)
    props: &'static [(&'static str, &'static str, bool)],
}

const SPECS: &[Spec] = &[
    Spec {
        tool: "propose_start_delegation",
        kind: "startDelegation",
        default_label: "Iniciar delegação",
        description: "Propõe criar e iniciar uma delegação por etapas para um agente, num contexto de projeto.",
        props: &[
            ("contextId", r#"{"type":"string","description":"id do contexto do projeto"}"#, false),
            ("title", r#"{"type":"string","description":"título curto da delegação"}"#, true),
            (
                "steps",
                r#"{"type":"array","items":{"type":"string"},"description":"passos curtos, na ordem"}"#,
                true,
            ),
        ],
    },
    Spec {
        tool: "propose_approve_step",
        kind: "approveStep",
        default_label: "Aprovar etapa",
        description: "Propõe aprovar a etapa que está aguardando aprovação numa delegação.",
        props: &[("delegationId", r#"{"type":"string"}"#, true)],
    },
    Spec {
        tool: "propose_adjust_step",
        kind: "adjustStep",
        default_label: "Pedir ajuste",
        description: "Propõe pedir ao agente que refaça a etapa atual com um ajuste.",
        props: &[
            ("delegationId", r#"{"type":"string"}"#, true),
            ("feedback", r#"{"type":"string","description":"o que ajustar"}"#, true),
        ],
    },
    Spec {
        tool: "propose_install_hooks",
        kind: "installHooks",
        default_label: "Instalar hooks",
        description: "Propõe instalar os hooks da Nyx no Claude Code.",
        props: &[],
    },
    Spec {
        tool: "propose_create_context",
        kind: "createContext",
        default_label: "Criar contexto",
        description: "Propõe criar um contexto de projeto (pasta raiz e nome).",
        props: &[
            ("name", r#"{"type":"string"}"#, true),
            ("rootPath", r#"{"type":"string","description":"pasta raiz do projeto"}"#, true),
        ],
    },
    Spec {
        tool: "open_panel",
        kind: "openPanel",
        default_label: "Abrir",
        description: "Propõe abrir uma aba do painel da Nyx.",
        props: &[(
            "tab",
            r#"{"type":"string","enum":["agents","delegate","usage","events","contexts","settings","chat"]}"#,
            true,
        )],
    },
];

/// Tool definitions for the Messages API. Every tool also takes a short `label`
/// (the confirmation button text in pt-BR).
pub fn tool_definitions() -> Value {
    let tools: Vec<Value> = SPECS
        .iter()
        .map(|s| {
            let mut props = Map::new();
            let mut required = vec![Value::String("label".into())];
            props.insert(
                "label".into(),
                json!({"type":"string","description":"texto curto do botão de confirmação, em português"}),
            );
            for (name, schema, req) in s.props {
                props.insert((*name).into(), serde_json::from_str(schema).expect("static schema"));
                if *req {
                    required.push(Value::String((*name).into()));
                }
            }
            json!({
                "name": s.tool,
                "description": s.description,
                "input_schema": {
                    "type": "object",
                    "properties": props,
                    "required": required,
                    "additionalProperties": false
                }
            })
        })
        .collect();
    Value::Array(tools)
}

/// Turns the `tool_use` blocks of a response into proposals. Unknown tool names and
/// non-object inputs are dropped.
pub fn actions_from_blocks(blocks: &[ContentBlock]) -> Vec<ProposedAction> {
    blocks
        .iter()
        .filter_map(|b| {
            let ContentBlock::ToolUse { id, name, input } = b else { return None };
            let spec = SPECS.iter().find(|s| s.tool == name)?;
            let mut args = input.as_object()?.clone();
            let label = args
                .remove("label")
                .and_then(|l| l.as_str().map(|s| s.trim().to_string()))
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| spec.default_label.to_string());
            Some(ProposedAction { id: id.clone(), kind: spec.kind.to_string(), label, args })
        })
        .collect()
}
