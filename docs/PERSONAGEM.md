# Nyx — ficha de personagem (v2)

Status: **personalidade, enquadramento e base visual (ref-03) aprovados**. A Nyx é um **modelo 3D anime (VRM)** feito no VRoid Studio e renderizado com three.js + three-vrm; pipeline em [AVATAR-3D.md](AVATAR-3D.md). Quando o modelo for aprovado, ele vira o canon e o que estiver aprovado vira **Travado**.
Nível de continuidade: **Tier 4 — asset determinístico**. Um único modelo 3D garante o mesmo rosto em todos os estados e ângulos. (Live2D e o estudo vetorial em `docs/design/` foram descartados.)

**Base visual aprovada**: [ref-03-base-gemini.webp](referencias/ref-03-base-gemini.webp) — referência de composição, pose, iluminação e personagem para o modelo 3D.

Referências de inspiração (**não são o design final**; não copiar traço a traço):
- [ref-01-retrato.png](referencias/ref-01-retrato.png) — direção de rosto, cabelo, fone e brinco.
- [ref-02-capuz.png](referencias/ref-02-capuz.png) — direção do moletom de capuz e do clima (telas de HUD ao fundo).

## 1. Identidade
- **Nome**: Nyx (deusa grega da noite; combina com a chuva noturna e o roxo).
- **O que é**: personagem original, mulher adulta (aparenta uns 25 anos), estilo anime em cel shading quase monocromático. É a operadora que vigia seus agentes.
- **Lema**: 常にそばに ("sempre ao seu lado"), da ref-02. Fica como textura de HUD, opcional.
- **Direitos**: original. Cada arte nova passa pelo teste de distância (§7).

## 2. Personalidade (aprovada)
**Calma, seca e séria, com brincadeiras ocasionais.**
- **Base**: fala pouco, direto, sem exclamação nem emoji. Dá o fato antes da opinião. Nunca bajula e nunca se desespera, nem quando dá erro.
- **Humor**: seco e raro, no máximo ~1 em cada 10 mensagens, nunca em erro nem em alerta de uso. É um comentário de canto de boca, não piada pronta.
- **Tratamento**: chama o usuário sempre de **"Dev"**. Sem outros apelidos.
- **Expressão base**: pálpebras meio baixas, boca neutra e o olhar de canto da ref-01. Quando sorri, é meio sorriso de um lado só.

| Situação | Exemplo |
|---|---|
| Etapa concluída | "Etapa 2 de 4 concluída. 3 arquivos alterados, testes passando. Aprova?" |
| Aguardando input | "Dev, o Claude Code em *projeto demo* parou. Quer uma resposta sua." |
| Erro | "Dev, falhou na etapa 3: build quebrou em `main.rs:42`. Log anexado." |
| Uso alto | "Semana em 82% do teto. Faltam 3 dias." |
| Brincadeira ocasional | "Tudo concluído. Nenhum agente pegou fogo hoje." |
| Abertura | "Online, Dev." / "De volta. 2 agentes rodando." |

**Voz (TTS)**: grave-médio, ritmo calmo, pouca variação de entonação e pausas curtas. O motor ainda não foi escolhido.

## 3. Silhueta e formas
- **Silhueta**: franja reta e pesada, cabelo longo e liso caindo à frente dos ombros, **fone over-ear grande com anel neon** e gola alta da jaqueta. Esses três juntos se reconhecem mesmo pintados de preto.
- **Formas**: retas e diagonais na roupa (gola, faixas neon em "L", peças técnicas); curvas suaves no rosto. Leitura: precisa e fria por fora, nada hostil.

## 4. Âncoras travadas
| Elemento | Regra |
|---|---|
| Cabelo | Preto (brilhos em #2A2236), liso, longo até o meio das costas, **franja reta e pesada até a linha dos olhos**, mechas laterais emoldurando o rosto (ref-01). Sem mecha colorida, sem rabo de cavalo, nunca curto nem claro. |
| Olhos | Violeta (#8B5CF6) com íris clara (#C084FC) e pálpebra superior pesada. Delineado fino. |
| Pinta | **Pinta pequena logo abaixo do olho esquerdo da personagem**, perto do canto externo (ref-01; na tela, aparece à direita quando ela está de 3/4 virada para a esquerda da tela). Nunca muda de lado. |
| Pele | Clara, acinzentada e quase sem saturação. Lábios neutros escuros, sem batom colorido. |
| Fone | Headphone over-ear preto, robusto, **com anel neon #C084FC em cada concha**. É o item-assinatura e nunca some. |
| Brinco | Pingente fino de corrente com um **cursor de terminal** (barrinha `▮` em neon #C084FC), **orelha esquerda**. Pisca devagar (~1 Hz) enquanto ela está ativa, apaga quando ela "dorme". Representa a personalidade: calma, sempre online e vigilante, com o humor seco de usar o próprio prompt como joia. (Substitui a cruz das referências.) |
| Jaqueta (padrão) | Tech jacket preta de **gola alta**, com placas e faixas neon em "L" no peito e nos ombros (ref-01). |
| Moletom (variante) | Moletom oversized preto com capuz, "AI" + código de barras no peito/ombro e faixa neon vertical na manga (ref-02). Uso: modo noite/foco. |
| Proporções | Adulta: rosto alongado, pescoço e ombros adultos. Nunca chibi nem infantil. Corpo em proporção natural, sem enfatizar (ver negativo). |
| Paleta | Só os tokens do app: #0D0B12, #1A1622, #8B5CF6, #C084FC, #E8E4F0, #8A8494. Proibido vermelho, verde e amarelo, inclusive no alerta. |

**Flexível**: expressão; intensidade do neon; jaqueta ou moletom; capuz para cima ou para baixo; presilha geométrica no cabelo (ref-02, opcional); mechas soltas.
**Negativo**: pose ou decote sexualizados, busto enfatizado, orelhas de animal, armadura, armas, maquiagem pesada, mecha colorida, visor no rosto, cópia de personagem conhecida.

## 5. Enquadramento (aprovado): total
Plano geral da cena, câmera levemente de 3/4 por trás e de lado:
- Nyx **sentada de corpo inteiro** numa cadeira gamer/de escritório, à mesa;
- **monitor(es) com telas de HUD** em roxo (como no fundo da ref-02) e teclado com as **mãos visíveis digitando**;
- **janela grande ao fundo**, com a cidade noturna desfocada e a chuva.

Cortes no painel: a cena inteira fica no topo do painel. No overlay e nos toasts, usa-se **outra câmera na mesma cena** (busto), sem arte nova.

## 6. Estados (detalhes técnicos em AVATAR-3D.md §3)
| Estado | Quando | Corpo | Detalhes | Loop |
|---|---|---|---|---|
| `intro` | Painel abre | Ver §7 | Anéis do fone acendem quando ela vira | Uma vez → `idle` |
| `idle` | Nada rodando | Respira devagar, pisca a cada 3–6 s, digita às vezes, olha a tela | Neon baixo | Lento e barato |
| `analyzing` | Agente rodando ou Nyx resumindo | Digita rápido, inclina para o monitor | Telas rolam; `ANALYZING...` | Médio |
| `alert` | Etapa concluída, input pedido ou erro | Para, gira a cadeira para a câmera, sobrancelha sobe | Anéis do fone e faixas pulsam em #C084FC + glitch ASCII de ~150 ms | Até você abrir |
| `speaking` | TTS falando (camada por cima dos outros estados) | — | Visemas pelo áudio (§8) | — |

Em 3D ela pode **girar a cadeira** de verdade na abertura e no alerta.

## 7. Abertura (~2,5 s)
De trás para frente: **cidade** (plano 2D desfocado) → **chuva** (canvas, atrás do vidro) → **cena 3D** (quarto, cadeira, Nyx, mesa, teclado, monitores) → **HUD** (React).
1. **0,0–0,4 s** — Escuro; a chuva entra com fade, os monitores acendem e projetam luz roxa nela; a câmera começa um dolly lento.
2. **0,4–1,4 s** — Nyx de costas/3/4 digitando; cabelo e brinco balançam (spring bones).
3. **1,4–2,0 s** — Aperta uma última tecla, faz um pequeno recuo (antecipação) e gira a cadeira para a câmera.
4. **2,0–2,5 s** — Os anéis do fone acendem, o HUD mostra `STATUS: ACTIVE`, ela diz "Online." (se a voz estiver ligada) e passa ao `idle`.

Clicar pula a abertura. Com "reduzir movimento" do Windows ativo, abre direto no quadro final. **ASCII na abertura**: a cena nasce toda em caracteres violeta e uma linha de scan a "resolve" para o 3D durante os passos 1–2 (ver [AVATAR-3D.md](AVATAR-3D.md) §4).

A chuva e a cena 3D **param totalmente** quando a janela é escondida ou minimizada (antes, a cena dissolve para ASCII); depois de ~60 s escondida, a cena é descarregada. Na bandeja, nada é renderizado.

## 8. Rig e voz
Modelo, animações, ASCII e orçamento: ver [AVATAR-3D.md](AVATAR-3D.md).
- **Piscar**: expressão `blink` automática a cada 3–6 s; às vezes pisca duas vezes seguidas.
- **Voz**: visemas VRM (`aa`, `ih`, `ou`, `ee`, `oh`) dirigidos pelo áudio do TTS; o mínimo é volume → `aa`. Não precisa de fonemas do motor.
- **Expressões**: `neutral` (pálpebra meio baixa), `relaxed`, `surprised` leve e `smirk` próprio (meio sorriso de um lado só).
- **Limites**: cabeça ±35°; cadeira gira até ~40° para a câmera. Olhos seguem o mouse só no `idle` (opcional).

## 9. O que produzir
1. **Modelo VRoid** da Nyx seguindo §3–§4 (franja, cabelo, olhos, pinta, jaqueta com faixas em L).
2. **Fone e brinco** no Blender (anéis e faixas como material emissivo).
3. **Cenário low-poly**: mesa, cadeira, teclado, monitores, parede/janela + imagem da cidade.
4. **Animações**: ociosa sentada, digitação normal e rápida, giro da cadeira.
5. **Atlas de glifos ASCII** (PNG) e o shader de mistura.
6. Variante moletom (opcional, depois): só a roupa.
**Teste de distância**: com o modelo aprovado, olhar miniatura, silhueta, retrato e descrição. Ela não pode lembrar pessoa real nem personagem existente.

### Âncora de prompt (para gerar referências 2D no Gemini, se precisar)
```text
Character anchor: Nyx, an original adult anime woman (reads mid-20s), long straight black hair to mid-back with heavy blunt bangs at eye level and face-framing side locks, half-lidded violet eyes with light violet irises, small beauty mark just below her LEFT eye near the outer corner, pale grayish skin, dark neutral lips, large black over-ear headphones with a glowing neon-violet ring on each ear cup, thin chain earring on her left ear with a small glowing neon-violet terminal-cursor bar pendant, black high-collar techwear jacket with neon violet L-shaped light strips on chest and shoulders.
Scene: full-body, seated in an office chair at a desk, hands on a keyboard, several monitors with violet HUD screens, large window behind with a blurred night city and rain.
Style: clean anime cel shading, near-monochrome dark palette (#0D0B12, #1A1622) with neon violet (#8B5CF6, #C084FC) as the only accent.
Avoid: childlike proportions, sexualized pose or outfit, emphasized chest, animal ears, visor, armor, weapons, colored hair streak, red/green/yellow, short or light hair, ponytail, mole on the right side.
```

## 10. Em aberto
1. **Quem modela no VRoid/Blender**: você mesmo ou um modelador VRM contratado.
2. **Motor de TTS**: local (ex.: Piper, gratuito e offline) ou nuvem. Basta entregar o áudio.
3. **Animações**: Mixamo ou captura própria.
