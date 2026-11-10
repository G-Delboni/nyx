# Spec — Noite 2 (2026-11-07): visual HUD, delegação, avatar no painel e chat

Base: a branch `worktree-fase-0` (Fase 0 + MVP base integrados e testados). O contrato da UI é o `app/src/api/contract.ts` (seções "Night 2"). Fora desta noite: TTS (a voz será escolhida amanhã), Orca, MCP local, updater/assinatura.

## 0. Decisões do usuário (2026-11-07, não reabrir)
| Tema | Decisão |
|---|---|
| Permissões da delegação | **Lista por contexto** (`ProjectContext.allowedTools` → `--allowedTools`). Ferramenta fora da lista = passo `blocked`, usuário decide. |
| Origem dos passos | **Os dois**: manual por padrão; com IA ligada no contexto, a Nyx (Haiku) propõe o plano a partir de um objetivo e o usuário edita. |
| Git | **Worktree por delegação** (`nyx/deleg-<id>`), **1 commit por passo aprovado**; no fim o usuário escolhe merge ou manter a branch. |
| Sessão | **Mesma sessão** (`--resume`). Ao chegar a **60% da janela de contexto**: a Nyx pede ao agente o `PASSAGEM.md` e abre uma sessão nova que começa lendo esse arquivo (equivalente headless do `/clear`; `nyx_claude::delegate::{needs_handoff, handoff_write_request, handoff_resume_request}`). |
| Concorrência | **1 delegação ativa por contexto**; contextos diferentes em paralelo. |
| Chave da API | **Variável de ambiente `ANTHROPIC_API_KEY`**. Sem a chave, a IA fica indisponível e tudo o mais funciona. |
| IA | Opt-in: toggle por contexto (`aiEnabled`) e por tarefa (`DelegationDraft.useAi`). Modelo: **Haiku**. Custo da Nyx numa linha separada no Uso. |
| Avatar | **Integrar agora** o protótipo 3D+ASCII com o modelo de exemplo; trocar pelo `nyx.vrm` depois é trocar o arquivo. |
| Chat | Spec própria (§4): **responde e propõe ações com cartão de confirmação**, **Haiku**, histórico **por contexto no SQLite**. |
| Updates | Sem updater por enquanto (Fase 4). |
| Orca | Depois da delegação. |
| Visual | A direção escolhida pelo usuário entre as 3 de `prototypes/panel-hud/` (§1). |

## 1. Visual HUD no painel (design: Fable → Claude → Fable)
- Aplicar a direção escolhida ao painel real, sem mudar o comportamento. O Fable é o dono temporário dos arquivos visuais: `app/src/styles.css`, `app/src/components/**`, `app/src/App.tsx` e o JSX/classes de `app/src/tabs/*`. Ele não mexe na lógica nem na API (`app/src/api/**`, `app/src/state.ts`, `app/src/hooks/**`).
- Componentes reutilizáveis que o C vai usar nas abas novas: `Panel`, `Card`, `StatusBadge`, `SegmentedBar`, `Button` (primário, fantasma, perigo-roxo), `Field`, `Toggle`, `Tabs`, `Microtext`, `EmptyState`, `ConfirmCard`. Documentar em `app/src/components/README.md`.
- O topo reserva o espaço do `<NyxAvatar>` (§3), com o fallback estático da direção escolhida.
- Fontes locais (@fontsource), nada de CDN. Regras: sem vermelho, verde ou amarelo; AA; animação leve e só com a janela visível; `prefers-reduced-motion`.
- Verificação dupla: o Fable implementa → o Claude revisa no navegador (mock, 420×640 e 380×520) e corrige → o Fable verifica as correções.

## 2. Delegação (Fase 2)
**Fluxo**: Delegar → escolher o contexto → título + passos (ou objetivo → `plan_delegation`) → `create_delegation` (draft) → `start_delegation`.
O núcleo então:
1. Cria um worktree em `<root do contexto>\.nyx\worktrees\<id>` com `git worktree add -b nyx/deleg-<id>` a partir da branch atual. Contexto sem git = erro claro ao criar.
2. Para cada passo, monta o `StepRequest` (`cwd` = worktree, instruções + referências do contexto, `--allowedTools` do contexto, `--resume` da sessão), roda `claude` com `nyx_claude::delegate::build_command`, lê o stdout com `parse_stream_line` (→ `liveActivity` + `nyx://delegation`) e fecha com `parse_stream`.
3. No fim do passo: `checkpoint` com o relatório (título + ≤ 5 linhas). Com IA ligada, mais o `aiSummary` (Haiku, ≤ 5 linhas). Toast de prioridade alta "Etapa 2 de 4 concluída. Aprova?" com o botão **Aprovar** e o clique abrindo o painel na delegação.
4. **Aprovar**: `git add -A && git commit -m "<título do passo>"` no worktree (mensagem em inglês, sem co-autoria da Nyx), e depois o próximo passo. No último, `done` + resumo final curto.
5. **Ajustar**: o mesmo passo de novo, com `--resume` e o feedback do usuário (`attempts += 1`).
6. **Passagem**: se `needs_handoff(outcome)`, roda `handoff_write_request`, e o passo seguinte usa `handoff_resume_request` (`handoffs += 1`; status `handoff` durante a troca).
7. **Bloqueio**: se o stream indicar negação de permissão para uma ferramenta fora da lista → `blocked` com o motivo. O usuário escolhe adicionar a ferramenta ao contexto e refazer, ou cancelar.
8. **Cancelar**: mata o processo (e a árvore de processos) e mantém o worktree. **Finish**: `merge` = merge `--no-ff` na branch base, remove o worktree e mantém a branch; ou só mantém tudo.
- Persistência: tabelas `delegations` e `delegation_steps` (migração v2). Ao reiniciar o app, um passo `running` sem processo vira `failed`, com a opção de refazer.
- Segurança: nunca rodar sem a lista do contexto; nunca `--dangerously-skip-permissions`; o executável `claude` vem do PATH (configurável depois).
- **Testes sem rodar o Claude real**: um executável falso (`crates/nyx-fakeclaude`, só para teste) que imprime fixtures de stream-json conforme os args; o núcleo aceita `NYX_CLAUDE_EXE` para apontar para ele. Repositório git temporário para testar worktree, commit e merge.
- **UI** (aba nova **Delegar**, entre Agentes e Uso): lista de delegações do contexto; editor de rascunho (passos reordenáveis; "Pedir plano à Nyx" quando a IA estiver disponível); a tela da delegação com os passos (status, relatório, resumo da IA, custo e % de contexto), a atividade ao vivo e as ações Aprovar / Ajustar (campo de texto) / Cancelar / Finalizar (merge ou manter). Na aba Contextos: editar `allowedTools` (lista com presets "Só leitura", "Editar arquivos", "Editar + testes npm"), `references` e `aiEnabled`.

## 3. Avatar no painel
- Portar `prototypes/avatar-ascii/index.html` para `app/src/avatar/` como `<NyxAvatar state="idle|analyzing|alert" visible />`: three.js + @pixiv/three-vrm + o pass ASCII; o modelo de exemplo fica em `app/public/models/sample.vrm` (fora do git, como hoje) e o código troca de modelo por constante.
- Mapeamento: `Snapshot.status` → `active`→idle, `analyzing`→analyzing, `alert`→alert. A abertura (intro) roda quando o painel abre.
- Desempenho (AVATAR-3D.md §5): import dinâmico (`React.lazy`) só com a janela aberta; 30 fps parado / 60 fps nas transições; para ao esconder (`visibilitychange`); descarte do contexto WebGL no unmount; `pixelRatio` ≤ 1,5. Sem o modelo, ou se o WebGL falhar → o fallback estático do visual. "Reduzir movimento" → sem intro e 15 fps.
- Regras fixas do CLAUDE.md: dedos fora do teclado e monitor sem tapar o rosto. Manter `window.__nyx.checkFingerClearance`/`checkMonitorOverlap` em dev e rodar as duas checagens no novo enquadramento do topo do painel.
- Dependências: `three@0.180` e `@pixiv/three-vrm@3.5.5` via npm (o orquestrador adiciona).
- É design (cena 3D) → verificação dupla com o Fable.

## 4. Chat com a Nyx
- Aba **Nyx** (ou painel deslizante, conforme o visual) com o histórico do contexto atual (`get_chat`) e uma caixa de texto. Sem a chave: o estado vazio explica como definir a `ANTHROPIC_API_KEY` (no tom da Nyx).
- O núcleo monta o pedido ao Haiku com: o system prompt da personagem (PERSONAGEM §2: seca, curta, sem exclamação nem emoji; "Dev"); um snapshot local compacto (agentes, uso da semana, delegações ativas, últimos eventos do contexto); as últimas 20 mensagens do histórico. Nada de transcripts nem código vão para a API.
- **Ações**: tool use do Haiku com as ferramentas `propose_start_delegation`, `propose_approve_step`, `propose_adjust_step`, `propose_install_hooks`, `propose_create_context` e `open_panel`. A resposta vira `ChatAction` com `state: "proposed"`; **só roda com o clique** (`run_chat_action`), que chama o mesmo command da UI. Nada é executado direto pelo modelo.
- Persistência: tabela `chat_messages` (migração v2/v3), por `context_id` (null = geral); "Limpar conversa" apaga as do contexto.
- Custo: `costUsd` por resposta, somado em `AiStatus.weekCostUsd` e mostrado na aba Uso como a linha "Nyx (IA)".
- Biblioteca: novo crate **`crates/nyx-ai`** (dono: Agente B): cliente da Messages API (HTTP blocking com `ureq`, timeout, retries com backoff em 429/529, sem streaming), tipos de tool use, os prompts (chat, resumo de passo, plano de passos) e o cálculo de custo. Testes com um servidor HTTP falso; **nenhuma chamada real à API nesta noite**. O modelo e os preços vêm da skill `claude-api`.

## 5. Donos dos arquivos nesta noite
| Quem | Arquivos |
|---|---|
| Orquestrador | `Cargo.toml` (raiz e crates), `Cargo.lock`, `app/package.json` e lock, `tauri.conf.json`, `capabilities/**`, `app/src/api/contract.ts`, `docs/specs/*.md` (exceto NOTAS-*) |
| A — núcleo (Opus) | `app/src-tauri/src/**` (delegação: runner, worktree, migrações, commands; chat: commands e persistência; eventos) |
| B — bibliotecas (Sonnet) | `crates/nyx-claude/**`, `crates/nyx-ai/**`, `crates/nyx-fakeclaude/**` (exceto os Cargo.toml) |
| C — UI (Sonnet) | `app/src/api/client.ts`, `app/src/api/mock*.ts`, `app/src/state.ts`, `app/src/hooks/**` e as abas novas `app/src/tabs/DelegateTab.tsx`, `app/src/tabs/ChatTab.tsx`, `app/src/delegation/**`, `app/src/chat/**` (usando os componentes do Fable) |
| Fable — design | `app/src/styles.css`, `app/src/components/**`, `app/src/App.tsx`, o JSX/estilo das abas existentes, `app/src/avatar/**`, `app/public/**` |

Ordem na UI: o Fable aplica o HUD e publica os componentes → o C constrói as abas novas em cima deles. Enquanto o Fable trabalha, o C faz a camada de API, o mock, o estado e a lógica (hooks), sem JSX final.

## 6. Pronto quando
- `cargo test --workspace`, `npm test` e `npm run build` passam; `npm run tauri build` gera o instalador.
- Delegação ponta a ponta com o `nyx-fakeclaude` num repositório temporário: 3 passos, 1 ajuste, 1 passagem simulada (`context_fill` ≥ 0,6), 1 bloqueio, commits por passo e merge final.
- Chat ponta a ponta com o servidor falso: pergunta → resposta com 1 ação proposta → confirmar → o command roda.
- O painel no visual escolhido, verificado nas duas camadas; o avatar no topo, passando as checagens das regras fixas e parando ao esconder; RAM com o painel escondido igual à de hoje (~10 MB após 60 s).
- Nada roda o `claude` de verdade nem chama a API de verdade nesta noite; o primeiro teste real é com o usuário, de manhã.
