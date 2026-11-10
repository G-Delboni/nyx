# Componentes do painel (direção E)

Tudo sai de `app/src/components` (`import { Card, Button } from "../components"`). Só os tokens do
`styles.css` (`bg`, `surface`, `surface-2`, `primary`, `neon`, `text`, `muted`, `line`); nada de
vermelho, verde ou amarelo. Texto nunca abaixo de 10 px. Estados de atenção usam intensidade de roxo:
`line` (quieto) → `primary` (rodando) → `neon` (precisa de você).

## Estrutura

```tsx
<Panel>                                   // casca de 420 px, coluna, poça roxa ao fundo
  <NyxScene snap line sceneMode contextName />  // topo (app/src/scene)
  <SegmentedBar percent={70} label="Uso semanal" marks={[80, 95]} />
  <Tabs items={[{ id: "agents", label: "Agentes", badge: 2 }]} value={tab} onChange={setTab} panelId="panel" />
  <PanelMain id="panel" role="tabpanel">...</PanelMain>
</Panel>
```

## Card
```tsx
<Card tone="waiting">                     // default | running | waiting | error | idle | neon | primary
  <CardTitle meta="semana">Por modelo</CardTitle>
  ...
</Card>
```
`waiting` pulsa devagar (para com `prefers-reduced-motion`). `cut={false}` tira os cantos cortados.

## Button
```tsx
<Button variant="primary">Aprovar</Button>   // um neon por tela
<Button>Editar</Button>                       // outline (padrão)
<Button variant="ghost" size="sm">lido</Button>
<Button variant="danger">Cancelar delegação</Button>   // destrutivo, em roxo
```

## Field / TextArea / Select / Toggle
```tsx
<Field label="Nome" value={v} onChange={...} hint="Opcional." />
<TextArea label="Passos" rows={5} />
<Select label="Modelo" value={m} onChange={...}><option value="">Padrão</option></Select>
<Toggle checked={on} onChange={setOn} label="Resumos com IA" hint="Haiku, opt-in." />
```
Todos já ligam `label` ao controle (`useId`).

## StatusBadge / DelegationBadge
```tsx
<StatusBadge status="waiting" />            // AgentSession.status
<DelegationBadge status="checkpoint" />     // Delegation.status
```

## Microtext
```tsx
<Microtext tone="neon" cursor>STATUS: ACTIVE</Microtext>
<Microtext>SCN 01 · CAM A</Microtext>
```

## EmptyState
```tsx
<EmptyState title="Delegar" action={<Button>Nova delegação</Button>}>
  Nenhuma delegação neste contexto.
</EmptyState>
```
Uma linha, no tom da Nyx (seca, sem exclamação).

## ConfirmCard
Substitui os botões que dispararam a ação (não é modal):
```tsx
<ConfirmCard confirmLabel="Sim, apagar" tone="danger" onConfirm={...} onCancel={...}>
  Apaga o contexto. As sessões continuam existindo.
</ConfirmCard>
```

## Stepper (aba Delegar)
```tsx
<Stepper current={1} items={[
  { title: "Criar worktree", state: "approved" },
  { title: "Rodar testes", state: "checkpoint", detail: <ul>...</ul> },
  { title: "Merge", state: "pending" },
]} />
```
`state` = `DelegationStep["status"]`.

## ChatBubble (aba Nyx)
```tsx
<ChatBubble role="nyx" meta="há 2 min · US$ 0,002" actions={<Button variant="primary">Iniciar delegação</Button>}>
  Dev, posso começar pela etapa 1.
</ChatBubble>
<ChatBubble role="user">Começa.</ChatBubble>
```

## NyxLine
```tsx
<NyxLine text={nyxLine(snap, delegations, { justOpened, now })} />
```
Digitação cyberpunk (spec no topo de `NyxLine.tsx`); re-digita só quando `text` muda; com
`prefers-reduced-motion` ou janela escondida, aparece pronto. Posicionada pela `.scene-say`.

## Slider
```tsx
<Slider label="Volume" min={0} max={1} step={0.05} value={v} format={formatVolume} onChange={setV} />
```
Trilha segmentada (20 células, como a barra de uso) preenchida em `primary` até o valor, thumb neon 8×14, valor em mono à
direita do rótulo. O `<input type="range">` nativo fica por baixo (teclado, foco visível, leitor de tela); só o visual muda.

## SpeechWave / NyxLine durante a fala
```tsx
<NyxLine text={caption} stepMs={typingStepFor(speech, caption.length)} instant={voltouDaFala} lead={<SpeechWave speech={speech} />} />
```
`SpeechWave` são 5 barras neon de 2 px que seguem o envelope da voz (`useSpeech()`, 20 Hz, escrita direta no `style`),
entram em 200 ms e somem 300 ms depois do áudio; com `prefers-reduced-motion` ficam paradas. A cena (`NyxScene`) monta isso
sozinha: durante a fala a NyxLine mostra `speechCaption(text)` (corta na frase/palavra, ≤ 110 chars) digitada no passo do
áudio; no fim a frase normal volta pronta (`instant`). Helpers puros em `app/src/scene/speech.ts`.
