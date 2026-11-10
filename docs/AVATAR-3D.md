# Nyx em 3D — VRoid + three.js + ASCII

Decisão (2026-11-06): a Nyx é um **modelo 3D anime (VRM)** feito no **VRoid Studio**, com acabamento no **Blender**, renderizado no app com **three.js + @pixiv/three-vrm** (shader toon MToon). Substitui o Live2D ([LIVE2D.md](LIVE2D.md), descartado) e o estudo vetorial (`docs/design/`).
Aparência e personalidade: [PERSONAGEM.md](PERSONAGEM.md). Base visual: [ref-03-base-gemini.webp](referencias/ref-03-base-gemini.webp).

## 1. Pipeline
| # | Etapa | Ferramenta (gratuita) | Saída |
|---|---|---|---|
| 1 | Modelar a Nyx: corpo, rosto, olhos, franja reta, cabelo longo, jaqueta | VRoid Studio | `nyx.vroid` |
| 2 | Pintar texturas próprias: olhos violeta de pálpebra pesada, pinta sob o olho esquerdo, pele acinzentada, faixas em L na jaqueta. É o que tira a "cara de VRoid padrão" | VRoid Studio (editor de textura) ou Krita | Texturas PNG |
| 3 | Exportar VRM 1.0 | VRoid Studio | `nyx.vrm` |
| 4 | Acessórios que o VRoid não faz bem: **fone over-ear com anéis emissivos**, **brinco de corrente com pingente de cursor de terminal (emissivo, pisca)**; materiais emissivos nas faixas neon; spring bones no cabelo e no brinco | Blender + add-on VRM (open source) | `nyx.vrm` final |
| 5 | Cenário 3D leve: mesa, cadeira, teclado, 3–4 monitores (telas como textura/HTML), parede/janela | Blender (low-poly) ou assets CC0 | `room.glb` |
| 6 | Animações: sentada digitando, digitando rápido, virar para a câmera, ociosa | Mixamo (gratuito, conta Adobe) retargetado para VRM, ou animação própria em `.vrma` | `.fbx`/`.vrma` |
| 7 | Runtime no app: carregar `room.glb` + `nyx.vrm`, tocar animações, expressões, olhar, sincronização labial | three.js + @pixiv/three-vrm (MIT) | Componente `<NyxScene state=… />` |

## 2. Cena
- **3D**: Nyx, cadeira, mesa, teclado, monitores (as telas usam textura de canvas/HTML com o HUD).
- **2D atrás**: cidade noturna desfocada em um plano de imagem (barato, sem modelar prédios) + **chuva em canvas** por cima, atrás do vidro.
- **Luz**: roxa forte dos monitores (point/rect light pela esquerda), contraluz fria da janela e ambiente bem baixo. Neon por material emissivo + bloom leve (opcional; desligável).
- **Câmera**: 3/4 de trás e de lado, igual à ref-03. Para o recorte de busto do overlay/toast, use **a mesma cena com outra câmera**.

## 3. Estados → animação e expressão
| Estado | Animação | Expressão VRM | Extra |
|---|---|---|---|
| `intro` | Câmera faz um dolly leve; ela digita, para e **gira a cadeira** para a câmera | `neutral` → leve `relaxed` | Anéis do fone acendem (emissivo 0 → 1) |
| `idle` | Loop sentada, digitação esporádica, respiração | `neutral` + piscar automático | Olhos podem seguir o mouse (lookAt) |
| `analyzing` | Digitação rápida, inclinada para o monitor | `neutral`, pálpebra mais baixa | Telas rolam |
| `alert` | Para, vira para a câmera | `surprised` leve | Neon pulsa |
| `speaking` | Por cima de qualquer estado | Visemas `aa`/`ih`/`ou`/`ee`/`oh` pelo áudio | Volume → `aa` é o mínimo; análise de frequência melhora |
| meio sorriso | Ocasional (brincadeira) | Blendshape próprio `smirk` (um lado só) | Criar no Blender |

Transições com crossfade de 0,3–0,5 s (AnimationMixer).

## 4. Camada ASCII (mistura com o 3D)
O ASCII é um **pass de pós-processamento na GPU** (shader), não o `AsciiEffect` do three.js, que desenha texto no DOM e pesa na CPU.
- **Como funciona**: a cena 3D é renderizada numa textura, dividida em células (ex.: 8×12 px). Cada célula vira um caractere de um atlas de glifos, escolhido pela luminância (` .:-=+*#%@`, com katakana opcional como textura). A cor sai da paleta (#8B5CF6/#C084FC sobre #0D0B12).
- **Mistura**: um uniform `uAscii` (0 = 3D puro, 1 = ASCII puro) e uma **máscara por camada**, que permite deixar só o fundo, só a Nyx ou só uma faixa da tela em ASCII. Varrer a transição com uma linha de scan (`uSweep`).

| Momento | Uso do ASCII |
|---|---|
| `intro` | A cena nasce **toda em ASCII** e uma linha de scan de cima para baixo "resolve" para o 3D (~0,8 s), enquanto ela digita |
| `idle` | Fundo (cidade) em ASCII bem sutil, com a Nyx e a mesa em 3D |
| `analyzing` | Glifos correm pelas telas dos monitores; leve chuva de caracteres atrás do vidro misturada à chuva normal |
| `alert` | Pulso rápido de glitch: a cena vira ASCII por ~150 ms e volta, junto com o neon |
| Fechar/esconder | A cena "dissolve" para ASCII antes de parar de renderizar |
| Modo leve | Retrato estático; o ASCII aparece só na abertura |

Custo: um pass extra de tela cheia, com 1 textura de glifos. Desligável nas configurações; com "reduzir movimento", só fica o ASCII estático do fundo.

## 5. Desempenho (o 3D é a parte mais cara do app)
- **Carregamento sob demanda**: three.js e os modelos só carregam quando o painel abre. Na bandeja, nada de 3D na memória.
- **Ao esconder**: para o loop na hora; depois de ~60 s escondido, **descarta** cena, texturas e contexto WebGL e volta ao consumo base.
- **Orçamento**: Nyx ≤ 40 mil triângulos, texturas ≤ 2048 px (atlas), cenário ≤ 30 mil triângulos, 1 luz com sombra no máximo. Meta: painel aberto < 250 MB e GPU baixa; em repouso, igual ao app sem 3D.
- **FPS**: 30 fps no `idle`/`analyzing`, 60 fps só na abertura e nas transições. `pixelRatio` limitado a 1,5.
- **Reduzir movimento** (Windows): sem abertura, sem dolly e com 15 fps.
- **Modo leve** nas configurações: troca a cena 3D por um retrato estático renderizado do próprio modelo.

## 6. Licenças (conferir antes do lançamento)
- VRoid Studio: gratuito; o modelo criado é seu.
- three.js e @pixiv/three-vrm: MIT. Add-on VRM do Blender: open source.
- Mixamo: gratuito, uso comercial das animações permitido nos termos da Adobe (não pode redistribuir as animações soltas). Conferir os termos atuais.
- Assets CC0 (ex.: Poly Haven): livres.

## 7. Em aberto
1. **Quem modela**: você mesmo no VRoid (curva de aprendizado de dias) ou um modelador VRoid/VRM contratado.
2. **Fone e brinco**: modelar no Blender ou procurar asset gratuito com licença compatível.
3. **Animações**: Mixamo (rápido, genérico) ou captura própria (ex.: webcam com ferramentas gratuitas de mocap) para a digitação e o giro ficarem com a cara dela.
