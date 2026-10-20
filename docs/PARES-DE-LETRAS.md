# Pares de letras na fonte do jogo (DTE de fonte)

Japonês diz em um código de 2 bytes o que o português diz em 2 ou 3 letras, e
cada letra traduzida ocupa um código inteiro. Os pares de letras ("de", "a ",
"ão") desenhados nos códigos que a tradução deixa livres põem **2 letras em
cada código**: a frase ocupa menos bytes, cabe mais letra por linha da caixa
de texto e mais letra no espaço fixo de um nome. É a técnica das traduções de
Tokimeki Memorial e Shadowrun (CD) pra caber o inglês no espaço do
japonês.

Vale pra jogo japonês de 16 bits cuja fonte o app entende (a mesma condição
das letras acentuadas: fonte de 1 bit por ponto, largura fixa, com o código
Shift-JIS na frente de cada desenho — `docs/PROCESSO-GBA-JAPONES.md`, seção 1).

## Como usar

```bash
romtranslate pares jogo.rtsproj      # depois de traduzir
romtranslate validate jogo.rtsproj
romtranslate reinsert jogo.rtsproj --policy skip
```

O `pares` conta os pares no texto já traduzido do projeto, grava a lista em
`tables/jis16.pares.json`, extrai de novo (a tabela ganha os pares que cabem)
e mostra o texto gravado sem e com eles: bytes, linhas das mensagens que o app
quebra, frases de espaço fixo que ocupam o espaço inteiro e o teto em letras
do pedido à IA. A reinserção desenha os pares na fonte da cópia de trabalho.

Desfazer: apagar `tables/jis16.pares.json` e extrair de novo (`romtranslate
extract`). As traduções não mudam: elas são texto, e a tabela é que decide os
bytes.

## O que o app faz

1. **Quais pares.** Os pares de caracteres seguidos do texto traduzido, o mais
   usado primeiro, maiúscula à parte ("De" e "de"), com o espaço de um lado
   ("a ", " o") e nunca atravessando um código do jogo (`[FF01]`). Entram os
   usados 20 vezes ou mais, até 640. No jogo medido, uma lista 3 vezes maior
   (975 pares na tabela) ganhou menos de 1%.
2. **Em que códigos.** Um código vira par se é desenho vazio em todas as
   fontes ou um **kanji que nenhum texto do jogo usa** — traduzido ou não, o
   que a extração acha no original. Kana, letra, número e sinal nunca: o
   teclado da tela de nomes mostra kana que nenhum texto tem. E só até onde o
   texto do jogo mostra códigos, sem o que fica fora da curva: um salto de
   1.024 códigos ou mais entre dois códigos que o texto mostra corta o resto
   (no jogo medido, o texto vai até o 3.798 e um kanji raro está no 6.912; a
   fala só alcança os 4.096 primeiros desenhos, e os pares postos acima do
   6.900 saíram como kanji).
3. **Só o que cabe em todas as fontes.** O jogo medido guarda duas fontes na
   mesma ordem (16x10 e 8x8): o código do par aparece nas duas. O par só entra
   se cabe na caixa de tinta de cada uma; senão, a fonte pequena mostraria o
   kanji de antes no lugar dele. Na de 8x8 do jogo medido cabem "a ", "e ",
   "s " e os pares de letras estreitas. Dos 40 pares mais usados, 14 ficaram
   de fora: "de", "do", "co" (duas letras redondas passam dos 9 pontos da
   grande) e "os", "as", "m " (passam dos 7 da pequena).
4. **Como o par é desenhado.** Com as letras da própria fonte ("ｄ" e "ｅ"),
   lado a lado, 1 coluna entre elas, centradas na caixa de tinta da fonte. A
   letra fica mais estreita **só tirando coluna repetida** (o traço de cima
   do "ａ" ocupa 5 colunas iguais): o desenho é o da fonte. Letra de diagonal
   ("v", "x", "A", "M") não tem coluna repetida e só faz par com letra fina.
   Com o espaço, a letra encosta no lado dela e o espaço leva 3 colunas.
5. **A gravação.** A tabela grava pelo par mais à esquerda (guloso). Com pares
   de 2 letras e as letras avulsas na tabela, isso **já é o menor número de
   códigos**: escolher pares sem sobreposição é escolher intervalos de mesmo
   tamanho, e pegar o que termina primeiro é o ótimo. Programação dinâmica não
   ganharia um código (o teste compara com ela em 2 mil textos sorteados).
   Duas exceções, em que a letra vai sozinha: logo depois de comando com
   parâmetro (a letra pode ser o último parâmetro dele, como no nome
   esticado "Lucas---") e no trecho alinhado com espaços (2 seguidos, como em
   `[FF06:0002]   Sim   Não`): ali cada opção fica na coluna em que a
   tradução a pôs, se o jogo põe o cursor em coluna fixa.
6. **A caixa de texto e o pedido à IA.** A quebra de linha do app conta
   códigos, não letras: um par ocupa um. O teto em letras do pedido à IA (o
   espaço fixo de um nome, a linha da caixa que o app não quebra) é o do texto
   típico: a extração grava na tabela a linha `# pares: L letras em C
   codigos`, medida no texto traduzido do projeto, e o teto é o espaço em
   códigos vezes L/C. Pode prometer mais do que um texto sem nenhum par cabe:
   a validação confere com os bytes de verdade, e a repergunta encurta na
   proporção do que a resposta anterior ocupou.
7. **A auditoria.** A releitura da cópia de trabalho lê o código do par como
   as letras dele, não como o kanji que o cabeçalho Shift-JIS do desenho
   ainda diz.

## Medido no jogo japonês de GBA (20/10/2026)

Mother 3 (Japão), 11.447 frases traduzidas, 675 mil letras:

| | sem pares | com pares |
|---|---|---|
| pares na tabela | — | 327 (de 640 na lista) |
| letras por código | 1,00 | 1,48 |
| bytes do texto traduzido | 1.454.904 | 1.004.110 (−31%) |
| linhas das falas que o app quebra | 33.718 | 25.830 (−23%) |
| falas com mais linhas que o japonês | 5.939 | 3.862 |
| nomes que ocupam o espaço inteiro | 321 | 32 |
| teto do pedido à IA nos espaços fixos, somado | 12.106 letras | 16.417 letras |

Sem os pares com espaço ("a ", " d"), seriam 1,29 letra por código: eles são
~40% do ganho.

Visto no emulador (`tools/gba-tela`): a primeira fala do jogo coube em 2
linhas em vez de 3, a cama do quarto em 1 página em vez de 2, e o menu de
estado ("Força", "Defesa", "Nenhum", "Sem nada") lê bem. Os nomes já gravados
continuam abreviados ("BotaGoma"): só uma rodada nova da IA com o teto maior os
escreve inteiros.

## Limites

- **Espaçamento irregular.** A fonte é de largura fixa: letra sozinha entre
  pares ocupa a célula inteira ("pregu i çoso"), e a palavra separada por um
  espaço dentro de par fica a ~4 pontos da seguinte, contra ~13 do espaço
  sozinho. Lê-se, mas não é uma fonte de largura variável.
- **Maiúscula muito estreitada** (F, S, T a 2 ou 3 colunas) é a parte mais
  feia.
- **Texto que a extração não acha.** O código vira par se nenhum texto
  achado usa o kanji dele. Texto que o app não lê (um menu montado por
  código, por exemplo) e usa esse kanji passa a mostrar o par.
- **Cursor de menu.** O trecho alinhado com espaços vai sem pares, mas a
  opção de menu escrita sem espaço duplo (posição por código do jogo) usa
  pares normalmente; se o jogo põe o cursor em coluna fixa, confira no
  emulador.
- Fonte que o app não entende (largura variável, 2 ou 4 bits por ponto, sem o
  código Shift-JIS na frente) fica sem pares, como fica sem acento.
