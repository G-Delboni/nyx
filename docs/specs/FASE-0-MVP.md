# Spec — Fase 0 + base do MVP (madrugada de 2026-11-07)

Escopo da noite: **Fase 0 completa** (bandeja, hooks do Claude Code, toast de conclusão) e a **base do MVP**: uso do Claude Code lido dos transcripts, SQLite, contextos básicos e painel funcional. Fora do escopo: avatar 3D (só um espaço reservado), visual de HUD, delegação (`claude -p`), IA/Haiku, Orca, MCP e PTY.

Decisões do usuário (2026-11-07):
- Toolchain: Rust e VS 2022 Build Tools instalados nesta noite.
- Hooks: **ninguém mexe em `~/.claude/settings.json`** nesta noite. O `nyx-hook install` é testado só em arquivos temporários; o usuário roda amanhã.
- Painel: funcional, com os tokens e as fontes do `CLAUDE.md`, sem iteração de design.
- Teto de uso: preset **Max 5x** (valor configurável; ver §6).

## 1. Layout do repositório e donos dos arquivos
Cargo workspace na raiz. Cada agente só escreve nos caminhos que são dele. Os arquivos compartilhados são do orquestrador; para mudar um deles, o agente pede.

| Caminho | Dono |
|---|---|
| `Cargo.toml` (workspace), `Cargo.lock`, `.gitignore`, `docs/specs/**`, `app/src/api/contract.ts`, `app/src-tauri/tauri.conf.json`, `app/src-tauri/capabilities/**`, `app/src-tauri/Cargo.toml`, `app/package.json` e lockfile | **Orquestrador** |
| `crates/nyx-claude/**` (lib: hook payloads, parser de transcript, custo, merge de settings) e `crates/nyx-hook/**` (bin chamado pelos hooks) | **Agente B — adapters** |
| `app/src-tauri/src/**` e `app/src-tauri/icons/**` (núcleo: bandeja, servidor de eventos, SQLite, toasts, watcher de transcripts, commands do Tauri) | **Agente A — núcleo** |
| `app/src/**` exceto `app/src/api/contract.ts`; `app/index.html`, configs de Vite/Tailwind/TS | **Agente C — UI** |

Dependências novas: o agente pede ao orquestrador (que edita o `Cargo.toml`/`package.json`). Quem precisa de uma função de outro dono usa o contrato (§2–§4) e, até ela existir, um stub local.

## 2. Contrato do evento (hook → núcleo)
- O Claude Code chama `nyx-hook.exe event <HookName>` (`Stop`, `SubagentStop`, `Notification`, `PreToolUse`, `SessionStart`, `SessionEnd`, `UserPromptSubmit`) com o JSON do hook no stdin.
- `nyx-hook` lê `%APPDATA%\Nyx\runtime.json` = `{"port": u16, "token": "<hex 32>", "pid": u32}` (escrito pelo núcleo ao subir, apagado ao sair) e faz `POST http://127.0.0.1:<port>/v1/events` com o header `X-Nyx-Token` e o corpo:
  `{"source":"claude-code","hook":"Stop","received_at":"<RFC3339>","payload":<stdin JSON cru>}`
- Regras do `nyx-hook`: timeout total de **1,5 s**; se a Nyx não estiver rodando, ou se der erro, sai com **código 0 em silêncio** (nunca bloqueia nem atrapalha o Claude Code); não escreve nada no stdout (o stdout de hook tem significado para o Claude Code). Log opcional em `%APPDATA%\Nyx\logs\hook.log` (rotação em 1 MB).
- O núcleo responde 202 e normaliza com `nyx_claude::normalize_hook(hook, &payload) -> NormalizedEvent`.

```rust
// crates/nyx-claude — tipos públicos (contrato)
pub enum EventKind { TaskDone, NeedsInput, Error, SessionStart, SessionEnd, PromptSubmitted, ToolUse }
pub struct NormalizedEvent {
    pub kind: EventKind,
    pub session_id: String,
    pub cwd: Option<String>,             // pasta do projeto
    pub transcript_path: Option<String>,
    pub title: String,                   // ex.: "Claude Code concluiu"
    pub detail: Option<String>,          // ex.: mensagem do Notification
    pub raw_hook: String,
}
pub fn normalize_hook(hook: &str, payload: &serde_json::Value) -> NormalizedEvent;
```

## 3. Contrato do uso (transcripts)
- Fonte: `~/.claude/projects/<pasta-codificada>/<session-id>.jsonl`. As linhas `type == "assistant"` têm `message.model`, `message.id`, `message.usage.{input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens}`, `timestamp`, `sessionId` e `cwd`. **Deduplicar por `message.id`** (a mesma mensagem aparece em várias linhas com o mesmo usage).
- O formato não é API estável: o parser fica isolado e versionado (`PARSER_VERSION`), ignora linhas desconhecidas sem falhar e tem fixtures de teste.

```rust
pub struct UsageRecord { pub message_id: String, pub session_id: String, pub cwd: Option<String>,
    pub model: String, pub ts: chrono::DateTime<chrono::Utc>,
    pub input: u64, pub output: u64, pub cache_write: u64, pub cache_read: u64 }
pub fn parse_line(line: &str) -> Option<UsageRecord>;
/// Lê a partir de `offset` (bytes) e devolve os registros + o novo offset (só linhas completas).
pub fn read_from(path: &std::path::Path, offset: u64) -> std::io::Result<(Vec<UsageRecord>, u64)>;
pub fn estimate_cost_usd(r: &UsageRecord) -> f64; // tabela de preços por família de modelo, versionada
pub fn projects_dir() -> std::path::PathBuf;        // ~/.claude/projects
```

## 4. Contrato UI ↔ núcleo (commands e eventos do Tauri)
Fonte da verdade: `app/src/api/contract.ts` (o orquestrador mantém). O Rust usa os mesmos nomes de campo em `camelCase` (serde `rename_all = "camelCase"`).
- Commands: `get_snapshot() -> Snapshot`, `get_events({limit, contextId?}) -> NyxEvent[]`, `get_usage({range: "session"|"today"|"week", contextId?}) -> UsageSummary`, `list_contexts()`, `save_context(ctx)`, `delete_context({id})`, `get_settings()`, `save_settings(s)`, `export_usage_csv({range}) -> string` (caminho do arquivo), `mark_event_read({id})`.
- Eventos do núcleo para a UI: `nyx://event` (payload `NyxEvent`), `nyx://usage` (payload `UsageSummary` da semana), `nyx://agents` (payload `AgentSession[]`).
- A UI tem um **modo mock** (`app/src/api/mock.ts`, ativo quando não existe `window.__TAURI_INTERNALS__`) para rodar no navegador com `npm run dev`.

## 5. Núcleo (Agente A)
- **Bandeja**: ícone + menu `Abrir painel`, `Não perturbe` (toggle), `Sair`. Clique esquerdo abre o painel. Fechar a janela só a esconde (o app continua na bandeja). Instância única (`tauri-plugin-single-instance`).
- **Servidor de eventos**: `axum` (ou `tiny_http`) em `127.0.0.1` com porta livre escolhida pelo SO; token aleatório por execução; escreve o `runtime.json`. Rejeita sem token (401) e corpos > 256 KB.
- **SQLite** (`rusqlite` bundled) em `%APPDATA%\Nyx\nyx.db`, com migrações versionadas (`PRAGMA user_version`). Tabelas: `events`, `usage` (PK `message_id`), `transcript_offsets` (path, offset, mtime), `sessions` (session_id, cwd, último estado, started_at, updated_at), `contexts` (id, name, root_path, instructions, color, created_at), `settings` (key, value JSON).
- **Watcher de transcripts**: `notify` com debounce de 2 s em `~/.claude/projects`, sem polling agressivo; no início, faz o catch-up pelos offsets salvos. Lê só os bytes novos (`nyx_claude::read_from`).
- **Status por sessão**: `SessionStart`/`UserPromptSubmit`/`PreToolUse` → rodando; `Notification` → aguardando você; `Stop` → concluído; `SessionEnd` → encerrado. Sessão sem evento há 30 min fica `idle`.
- **Contexto**: a sessão pertence ao contexto cujo `root_path` é prefixo do `cwd` (comparação sem diferenciar maiúsculas, com `\` e `/` normalizados).
- **Toasts** (`tauri-plugin-notification`): `Stop` → "Claude Code concluiu" + nome da pasta (prioridade alta); `Notification` → "Dev, o Claude Code em *<pasta>* quer uma resposta sua." (alta); uso em 80% e 95% do teto (média, uma vez por semana e por limiar). Textos no tom da `docs/PERSONAGEM.md` §2 (sem exclamação, sem emoji). O "não perturbe" segura as notificações de prioridade média e marca badge no ícone. Botões de ação no toast são desejáveis; se o plugin não der suporte no Windows, abrir o painel ao clicar já basta (registrar em `docs/specs/NOTAS-A.md`).
- **Autostart**: `tauri-plugin-autostart`, **desligado por padrão** (toggle em Configurações). Nunca ligar nesta máquina durante os testes.
- **Leve**: nada de loop ativo; tudo por evento. Medir a RAM ociosa no fim e anotar.

## 6. Uso e teto
- Visões: sessão atual (última sessão com evento), hoje (00:00 local), semana (7 dias corridos). Por contexto quando `contextId` vier.
- `UsageSummary`: tokens (in/out/cache), custo estimado em US$, nº de sessões e de mensagens, por modelo, e `ceiling` + `percent`.
- Teto: o plano não publica tetos em tokens, então o teto é um **custo estimado semanal em US$, configurável**. Preset `max5x` com valor inicial provisório (o Agente B propõe e documenta a base do número); a UI mostra "estimativa" ao lado. Calibração fica como pergunta para o usuário.
- Export CSV da visão escolhida para `%USERPROFILE%\Documents\Nyx\usage-<range>-<data>.csv`.

## 7. UI (Agente C)
Janela única ~420×640, dark only, tokens do `CLAUDE.md` (bg #0D0B12, surface #1A1622, primary #8B5CF6, neon #C084FC, text #E8E4F0, muted #8A8494), Rajdhani nos títulos e JetBrains Mono nos dados (fontes locais via `@fontsource`, nada de CDN, porque o app é local-first). Abas: **Agentes** (cards por sessão: pasta, contexto, status, tokens da sessão, última atividade), **Uso** (barras segmentadas sessão/hoje/semana, por modelo, botão CSV), **Eventos** (lista, não lidos em destaque), **Contextos** (CRUD simples), **Config** (teto, preset, não perturbe, autostart, caminho do banco). Topo com espaço reservado de 160 px para o avatar e o microtexto `STATUS: ACTIVE`. Sem animação contínua.

## 8. Pronto quando
- `cargo build --workspace` e `cargo test --workspace` passam; `npm run build` passa; `npm run tauri build` gera o instalador (ou, no mínimo, `tauri build --no-bundle`).
- Teste ponta a ponta sem tocar no settings real: subir o app, rodar `nyx-hook event Stop` com um JSON de exemplo no stdin, e ver o evento no banco, o toast e o card no painel.
- Uso real calculado dos transcripts desta máquina (só leitura) e conferido contra uma conta manual de uma sessão.
- `nyx-hook install --settings <arquivo temporário>` adiciona os hooks da Nyx **sem remover os do Orca**, com backup e `--dry-run`; `uninstall` remove só os da Nyx.

## 9. Regras para os agentes
- Responder e comentar em pt-BR; código e identificadores em inglês. Commits pequenos na branch do próprio worktree, com mensagem terminando em `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Não fazer push nem merge.
- Proibido: escrever em `~/.claude/settings.json` ou em qualquer arquivo de `~/.claude` (transcripts são **só leitura**), ligar o autostart, rodar `claude -p`, instalar coisas globais, apagar `%APPDATA%\Nyx` de outro agente (use `NYX_DATA_DIR` para apontar o banco/runtime de teste para uma pasta temporária).
- Builds: `CARGO_TARGET_DIR=C:\Users\dev\.nyx-target` (compartilhado; o lock do cargo serializa).
- Ao terminar: escrever `docs/specs/NOTAS-<A|B|C>.md` (≤ 30 linhas: o que fez, como testar, pendências, perguntas ao usuário).
