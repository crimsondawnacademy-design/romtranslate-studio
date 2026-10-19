# nds-tela — ver o jogo de DS traduzido sem abrir janela

Ferramenta de conferência do processo de tradução, fora do app: roda um jogo
de Nintendo DS num emulador sem janela, seguindo um roteiro de teclas e
toques, e grava fotos das duas telas. É irmã da `tools/gba-tela`.

*A headless Nintendo DS runner for checking a translated game: a key and
touch script in, screenshots of both screens out. Not part of the app.*

## Montar (uma vez por máquina)

```bash
tools/nds-tela/montar.sh
```

Baixa o melonDS 1.1 oficial (licença GPL-3.0) em `tools/nds-tela/.melonds/`,
compila só o núcleo (sem Qt/SDL, sem JIT, sem OpenGL) e liga o `roda.cpp`
nele. Precisa de `cmake`, `git` e `libpng` (Homebrew). Leva menos de um
minuto e ocupa uns 60 MB. Nada disso entra no git: o executável leva o
núcleo do melonDS (GPL-3.0) e fica só na máquina de quem montou.

O jogo liga com a BIOS livre que vem dentro do melonDS e boot direto: não
precisa de BIOS nem firmware de aparelho.

## Rodar

```bash
tools/nds-tela/rodar.sh <jogo.nds> <pasta-das-fotos> passo...
```

O jogo roda dentro de `sandbox-exec`: sem rede, e só escreve na pasta das
fotos. Passos:

| passo | o que faz |
|---|---|
| `N` | anda N quadros sem tecla (60 = 1 segundo) |
| `A`, `START`, `DOWN*6` | aperta e solta a tecla (n vezes) |
| `hold=RIGHT:30` | segura a tecla por N quadros |
| `touch=128,73` | toca a tela de baixo em X,Y (0–255, 0–191) e solta |
| `touch=128,73:30` | segura o toque por N quadros |
| `shot=nome` | foto das duas telas em `nome.png` (256×384, a de cima em cima) |

Teclas: `A B X Y L R START SELECT UP DOWN LEFT RIGHT`.

Opção antes dos passos: `--sav arquivo.sav` (o save do jogo; o original só é
lido). O que o jogo gravar durante o roteiro vai pra `jogo.sav` na pasta das
fotos, e serve de `--sav` na próxima rodada.

Exemplo: ligar, pular a abertura e fotografar o menu.

```bash
tools/nds-tela/rodar.sh jogo.nds ~/fotos 600 START 120 shot=titulo START 120 shot=menu
```

Roda bem mais rápido que o aparelho (uns 900 quadros em 3 s).

Fotos e saves são do jogo: ficam fora do repositório.
