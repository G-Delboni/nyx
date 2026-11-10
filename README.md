# Nyx

Aplicativo de bandeja para Windows que supervisiona agentes de código, como o Claude Code. Ele avisa quando uma sessão termina ou trava, mostra o uso da semana, e delega tarefas em etapas que só avançam com a sua aprovação. A Nyx é também uma personagem: um avatar 3D que fala o que está acontecendo.

Feito com Tauri 2 (Rust) no núcleo e React com TypeScript na interface.

> **Estado do projeto:** MVP em desenvolvimento, sem versão publicada. O modelo 3D da personagem não está no repositório (veja "Avatar").

## O que faz

- **Acompanha as sessões.** Um hook instalado no Claude Code envia os eventos (fim de turno, pedido de permissão, uso de ferramenta) para o núcleo, que mostra o estado de cada sessão e dispara um aviso quando algo precisa de você.
- **Delega em etapas.** Uma delegação é uma lista de passos. Cada passo roda o agente em modo não interativo dentro de um git worktree separado e para num checkpoint, com um resumo, até você aprovar o próximo.
- **Conversa e propõe.** Com a IA ligada (opcional), a Nyx responde no chat e pode propor ações. As ferramentas do modelo só geram propostas: nenhuma ação é executada sem aprovação.
- **Fala.** Os avisos podem ser lidos em voz alta, por Azure Neural TTS ou pela voz do Windows como reserva.
- **Mostra o uso.** Tokens por sessão, por dia e por semana, com exportação em CSV.

## Arquitetura

```
Claude Code ──hooks──▶ nyx-hook (binário pequeno) ──▶ núcleo (Tauri, Rust, SQLite)
                                                        │
        nyx-claude: normaliza eventos de hook           ├─▶ interface (React): agentes, delegar, uso, chat
                    e roda o agente em worktrees        ├─▶ nyx-ai: API da Anthropic (chat, resumos, planos)
                                                        └─▶ nyx-voice: fala (Azure ou Windows) + boca do avatar
```

| Caminho | Conteúdo |
|---|---|
| `app/src-tauri/` | Núcleo: sessões, delegações, chat, uso, avisos e banco SQLite |
| `app/src/` | Interface em React: abas, cena 3D, cliente da API e simulações para desenvolvimento |
| `crates/nyx-claude/` | Leitura dos eventos de hook e execução do agente em modo não interativo |
| `crates/nyx-hook/` | Instalador do hook, que mescla a configuração sem apagar a que já existe |
| `crates/nyx-ai/` | Cliente da API da Anthropic, com ferramentas que só propõem ações |
| `crates/nyx-voice/` | Síntese de fala, normalização de texto, cache e fila de reprodução |
| `crates/nyx-fakeclaude/` | Agente simulado, usado nos testes no lugar do real |
| `docs/` | Especificações do MVP e a ficha da personagem |

## Decisões de engenharia

- **Aprovação humana por construção.** O modelo nunca executa: ele devolve propostas, e o núcleo só age depois de um clique. O texto de cada passo é tratado como dado, e há testes com passos maliciosos.
- **Isolamento por worktree.** Cada delegação trabalha numa cópia própria do repositório, então um passo ruim não mexe na sua árvore de trabalho.
- **Contrato primeiro.** A interface e o núcleo conversam por um contrato tipado (`app/src/api/contract.ts`), com uma simulação completa do núcleo para desenvolver a interface sem o Rust.
- **Testável sem rede.** O agente simulado e os clientes falsos permitem rodar os testes sem chamar nenhuma API.
- **IA opcional.** O aplicativo funciona sem chave de API; chat, resumos e planos só ligam quando configurados.

## Rodar

Requer Rust, Node.js 20 ou mais novo e os pré-requisitos do Tauri 2 para Windows.

```powershell
cd app
npm install
npm run tauri dev
```

Só a interface, com o núcleo simulado:

```powershell
cd app
npm run dev
```

## Testes

```powershell
cd app
npm install
npm run sidecar        # gera o binário do hook que o núcleo embute
npm test
cd ..
cargo test --workspace
```

## Avatar

A cena usa um modelo VRM, que não está versionado. Coloque um arquivo `.vrm` em `app/public/models/nyx.vrm` para ver a personagem; sem ele, a cena abre sem o avatar. A aparência e o tom de voz estão descritos em `docs/PERSONAGEM.md`.

## Limitações conhecidas

- Só Windows.
- Só o Claude Code tem adaptador; outros agentes de linha de comando ainda não.
- A voz por Azure exige uma chave sua, guardada fora do repositório.

## Licença

MIT. Veja [LICENSE](LICENSE).
