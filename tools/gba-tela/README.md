# gba-tela — ver o jogo traduzido sem abrir janela

Ferramenta de conferência do processo de tradução, fora do app: roda um jogo
de GBA num emulador sem janela, seguindo um roteiro de teclas, e grava fotos
da tela, cópias da memória e estados salvos. É assim que a tradução é vista
rodando sem tocar na tela de ninguém.

*A headless GBA runner for checking a translated game: a key script in,
screenshots, memory dumps and save states out. Not part of the app.*

## Montar (uma vez por máquina)

```bash
tools/gba-tela/montar.sh
```

Baixa o mGBA 0.10.5 oficial (licença MPL-2.0) em `tools/gba-tela/.mgba/`,
compila só a biblioteca e liga o `roda.c` nela. Precisa de `cmake`, `git` e
`libpng` (Homebrew). Nada disso entra no git.

## Rodar

```bash
tools/gba-tela/rodar.sh <jogo.gba> <pasta-das-fotos> passo...
```

O jogo roda dentro de `sandbox-exec`: sem rede, e só escreve na pasta das
fotos. Passos:

| passo | o que faz |
|---|---|
| `N` | anda N quadros sem tecla (60 = 1 segundo) |
| `A`, `START`, `DOWN*6` | aperta e solta a tecla (n vezes) |
| `hold=RIGHT:30` | segura a tecla por N quadros |
| `shot=nome` | foto da tela em `nome.png` |
| `dump=nome` | memória (EWRAM, IWRAM, VRAM) em `nome-*.bin` |
| `save=nome` | estado salvo em `nome.ss` |
| `poke=0x02001234:5:1` | escreve um valor (1, 2 ou 4 bytes) na memória |

Opções antes dos passos: `--sav arquivo.sav` (o save do jogo, copiado pra
pasta das fotos) e `--estado arquivo.ss` (começa de um estado salvo, em vez
de ligar o jogo).

`folha.py` junta fotos lado a lado, ampliadas:

```bash
python3 -I tools/gba-tela/folha.py folha.png foto1.png foto2.png
```

Fotos e estados são do jogo: ficam fora do repositório.
