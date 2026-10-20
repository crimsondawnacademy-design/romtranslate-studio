# Jogo japonês de GBA → português: o processo

O caminho que levou o primeiro RPG japonês medido (32 MiB, ~12 mil frases)
a 100% de texto em português, validado no emulador. Vale pra outro jogo
japonês de GBA: cada passo diz o que o app faz sozinho e o que pede olho.

Tudo aqui roda pela janela do app ou pelo CLI (`romtranslate`), com o mesmo
core por trás. Na janela, o **passo a passo** no topo da tela do projeto
segue esta ordem (extrair → custo → traduzir → imagens → reinserir →
auditoria → patch), diz o que falta em cada passo e em que painel o erro dele
aparece. Cada seção abaixo diz o botão e o comando.

## 1. Projeto e extração

```bash
romtranslate create jogo.gba jogo.rtsproj --source ja --target pt-BR
romtranslate extract jogo.rtsproj
```

A extração reconhece sozinha, pela estrutura do arquivo (sem saber que jogo
é):

- **texto em códigos de 16 bits na ordem do JIS** (`kuten`), com a ordem da
  fonte do próprio jogo quando ela guarda o código Shift-JIS de cada desenho
  (sem isso, todo kanji sai trocado);
- **bancos de mensagens** (diretório → tabelas de offsets → texto), salas em
  diretório misto, **salas com a tabela embutida** num arquivo de pares
  (offset, tamanho) ou soltas com ponteiro de ROM: a frase de banco **pode
  crescer** (o bloco é remontado e reapontado);
- comandos do jogo e quantos parâmetros cada um leva, a letra que um comando
  desenha (vira texto traduzível), o código que escreve um nome antes da
  frase (conta na largura da linha) e o espaço do JIS dentro de mensagem;
- a **largura da caixa de texto** e a quebra de linha do jogo: nos bancos com
  caixa que rola, o app quebra as linhas da tradução sozinho.
- a **fonte do texto**, quando o cartucho guarda cada desenho com o código
  Shift-JIS na frente: o formato do desenho (1 bit por ponto, a largura da
  linha pelo desenho de "｜" e a ordem dos bits pela haste de "Ｌ", "Ｆ", "Ｐ")
  e os códigos de desenho vazio que nenhum texto usa e que não passam do
  maior código que o texto do jogo mostra. Neles a tabela ganha as letras
  acentuadas (á é í ó ú â ê ô ã õ ç à, maiúsculas e outras que a fonte do
  app compõe), e a reinserção as desenha em todas as fontes nessa ordem:
  a letra base da própria fonte, o sinal da fonte do app.

Alfabeto próprio depois de um comando (o do "Sr. Saturno" no jogo medido):
ensine numa tabela `tables/jis16@<COMANDO>.tbl` e extraia de novo. Na
janela: painel "Alfabeto depois de um comando" (aparece em jogo japonês de
16 bits), com o comando e as letras dos códigos 1, 2, 3...; "Guardar e
extrair de novo" grava a tabela e extrai.

Na janela, a extração é o botão "Extração estruturada". O Escanear (scanner
genérico, de descoberta) tem região de início e fim em hexadecimal e a
escolha do `.tbl`; a região, a codificação e a tabela do último scan ficam no
projeto (`scan.json`), e a CLI usa o mesmo (`scan --region`).

## 2. Medir antes de traduzir

```bash
romtranslate estimate jogo.rtsproj          # tamanho da rodada, sem gastar
romtranslate reinsert jogo.rtsproj --policy skip
romtranslate audit jogo.rtsproj             # quanto do jogo segue em japonês
```

O `audit` relê a cópia gravada com a mesma extração e conta, caractere a
caractere, o japonês que sobrou e por quê (sem tradução, com erro, não
gravado, fora do projeto).

Na janela: painel "Custo da rodada" ("Calcular gasto", para qualquer
provedor: chamadas e tokens, e dólares com preço) e painel "Auditoria"
(motivos, regiões onde sobrou mais, imagens sem conferir e os primeiros
trechos — clicar num abre a frase no editor).

## 3. Traduzir

- Pela IA das configurações: `romtranslate translate jogo.rtsproj`.
- Por fora (outro modelo, uma pessoa, sem gastar cota):
  `estimate --pedidos lotes.json` grava os lotes exatos da rodada; a resposta
  volta como TSV (`id<TAB>tradução`) pelo `apply`, com a validação de sempre.

Na janela, no painel "Tradução": "Traduzir pendentes" (com cancelar; depois
de cancelar ou de a IA parar, o botão vira "Retomar" e manda só o que falta),
"Traduzir de novo as com erro" e, em "Traduzir por fora", "Exportar os lotes
(JSON)" e "Importar traduções (TSV)". Chamadas ao mesmo tempo e reperguntas
das que não cabem ficam no painel "Provedor de IA" (configuração do app,
que a CLI também lê).

**Acento depois.** Se a tradução foi feita antes de a fonte do jogo ganhar as
letras acentuadas (a extração desenha as que faltam em códigos vazios da
fonte), `romtranslate acentuar jogo.rtsproj` manda à IA a tradução pronta e
grava só a resposta que, sem os acentos, é idêntica à de antes: nenhuma
palavra, código ou linha muda. A frase com erro de grafia de propósito
costuma ser recusada (a IA "corrige"): acentue essas à mão, por TSV.

**Pares de letras depois.** Com a tradução feita, `romtranslate pares
jogo.rtsproj` desenha na fonte do jogo os pares de letras mais usados ("de",
"a "), em kanji que nenhum texto do jogo usa: 2 letras por código. No jogo
medido, o texto traduzido encolheu 31% e as falas, 23% das linhas. Como
funciona e os limites: `docs/PARES-DE-LETRAS.md`.

Regras que a validação cobra: os códigos do jogo (`[FFxx]`, `[FFxx:pppp]`)
ficam todos; letra acentuada só onde a tabela a tem — com a fonte entendida
(seção 1), o pedido à IA lista as letras e o TSV pode ter acento; a letra
que a fonte não ganhou o app troca pela sem acento na resposta da IA (no
TSV, é erro); frase de espaço fixo cabe no espaço; linha que passa da caixa
é erro onde o app não quebra sozinho.

Lixo lido como texto: `not-text <ids>` (ou a IA responde `<NOT_TEXT>`). Fica
marcado mesmo extraindo de novo. Na janela: "Não é texto (é dado)" no editor
da frase, e "Marcar essas N como não-texto" no resumo da validação para as
traduções sem nenhuma letra (`not-text --kind no_letters`).

## 4. Texto desenhado (imagens)

```bash
romtranslate pictures jogo.rtsproj                        # folhas em imagens/
romtranslate read-pictures jogo.rtsproj <id> --preview    # folha de trechos
romtranslate read-pictures jogo.rtsproj <id> [--leitura leitura.json]
romtranslate pictures jogo.rtsproj --sem-texto <ids>      # conferidas sem japonês
```

Na janela, painel "Texto em imagem": "Procurar imagens com letra", marcar as
miniaturas e "Ler as marcadas com a IA", "Ver a folha de trechos"
(`--preview`), "Ler de um arquivo" (`--leitura`, uma imagem por vez) e
"Marcar sem texto" (`--sem-texto`).

O app acha o alinhamento real dos quadros e se eles vêm em sprites (a linha
"aperte um botão" do aviso de saúde é uma fila de sprites 4×2). A leitura
pode ser da IA ou de um arquivo; a tradução segue como frase, e a reinserção
apaga o japonês (furigana inclusive) e escreve o português. Olhe as folhas
em `imagens/`: o `audit` cobra cada imagem até ser lida ou marcada.

Imagem que a tela monta por **mapa de quadros** aparece na lista como
`map-<posição do mapa>` ("mapa" no `pictures`): o app acha o mapa cru (32
entradas de 16 bits por linha, o fundo de texto do GBA) e de onde ele conta
os quadros, e a folha já sai montada, legível. Leia essa folha, não a dos
mesmos quadros na ordem guardada: ao ler a montada, as frases da outra que
caem nos quadros que o mapa mostra saem do projeto (a memória de tradução
devolve o que já estava traduzido). Na reinserção, cada célula redesenhada
ganha um quadro só dela; quadro repetido no desenho ou mostrado por outro
mapa achado não muda — se faltar quadro, a frase fica no original com o
motivo.

## 5. Reinserir, auditar, repetir

```bash
romtranslate validate jogo.rtsproj
romtranslate reinsert jogo.rtsproj --policy skip
romtranslate audit jogo.rtsproj
```

Pronto quando: `validate` sem erro, `audit` com 0 caracteres japoneses e
nenhuma imagem sem conferir.

## 6. Ver rodando

`tools/gba-tela` roda o jogo num emulador sem janela, com roteiro de teclas,
fotos, cópia da memória e estados salvos (ver o README de lá). A volta
mínima: aviso de saúde, título, telas de nomes e de ajustes, primeira fala,
um menu, **uma batalha**, uma loja ou um baú. No jogo medido, a batalha veio
de trocar o número da sala na RAM antes de passar por uma porta e pôr o
personagem ao lado de um inimigo (a posição também está na RAM): ache esses
endereços comparando duas cópias da memória com o personagem andando.

Cuidado com estado salvo: ele guarda a memória de vídeo; tela carregada de um
estado mostra os desenhos da versão em que ele foi gravado.

## 7. O que costuma sobrar

- **Imagem montada por mapa de quadros** (painel de cores da tela de ajustes
  no jogo medido): resolvida pelo app desde 17/10 (seção 4). O mapa achado
  sem texto em japonês (fundo, ilustração, créditos em inglês) é marcado com
  `--sem-texto`, como qualquer imagem; no jogo medido foram 11 de 13.
- **Tela montada por código ou por sprites de tamanhos misturados** (o caixa
  eletrônico no jogo medido, `img-01afc010`): não há mapa cru pra achar; fica
  em japonês e o `audit` cobra a imagem como "sem conferir".
- **Mapa que a busca não acha** (sem fila de quadros seguidos, comprimido ou
  com outro formato) usando os mesmos quadros de um mapa achado: vê o mesmo
  desenho novo nos quadros escritos no lugar; um quadro que o redesenho
  tomou pra outra célula aparece trocado nele. Confira no emulador as telas
  vizinhas (no jogo medido: as telas de nomes, que dividem o bloco de quadros
  com o painel de ajustes).
- **Teclado da tela de nomes** (hiragana/katakana): é entrada, não texto.
- **Acento no texto traduzido antes da fonte ganhar as letras**: a tabela
  passa a escrever acento, mas a tradução que já está no projeto foi gravada
  sem; só uma nova rodada (ou uma correção) põe o acento. No jogo medido, a
  fala alcança só os 4.096 primeiros desenhos da fonte (o código acima disso
  dá a volta): é por isso que as letras ficam abaixo do maior código que o
  texto do jogo mostra. Fonte de largura variável, de 2 ou 4 bits por ponto,
  ou sem o código Shift-JIS na frente de cada desenho, fica sem acento.
- **Campos de espaço fixo** (nomes de inimigo, item, PSI): a tradução cabe no
  espaço do maior nome da tabela; sai abreviada. Com os pares de letras
  (`docs/PARES-DE-LETRAS.md`) o espaço comporta ~1,4 vez mais letras, mas o
  nome já gravado só cresce numa rodada nova da IA.
- **Tradução em inglês ou abreviada** de rodadas antigas: procure no banco do
  projeto e refaça (no jogo medido, um lote inteiro do roteiro tinha voltado
  em inglês).
