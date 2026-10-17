# Jogo japonês de GBA → português: o processo

O caminho que levou o primeiro RPG japonês medido (32 MiB, ~12 mil frases)
a 100% de texto em português, validado no emulador. Vale pra outro jogo
japonês de GBA: cada passo diz o que o app faz sozinho e o que pede olho.

Tudo aqui roda pelo CLI (`romtranslate`), com os mesmos passos da janela.

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

Alfabeto próprio depois de um comando (o do "Sr. Saturno" no jogo medido):
ensine numa tabela `tables/jis16@<COMANDO>.tbl` e extraia de novo.

## 2. Medir antes de traduzir

```bash
romtranslate estimate jogo.rtsproj          # tamanho da rodada, sem gastar
romtranslate reinsert jogo.rtsproj --policy skip
romtranslate audit jogo.rtsproj             # quanto do jogo segue em japonês
```

O `audit` relê a cópia gravada com a mesma extração e conta, caractere a
caractere, o japonês que sobrou e por quê (sem tradução, com erro, não
gravado, fora do projeto).

## 3. Traduzir

- Pela IA das configurações: `romtranslate translate jogo.rtsproj`.
- Por fora (outro modelo, uma pessoa, sem gastar cota):
  `estimate --pedidos lotes.json` grava os lotes exatos da rodada; a resposta
  volta como TSV (`id<TAB>tradução`) pelo `apply`, com a validação de sempre.

Regras que a validação cobra: os códigos do jogo (`[FFxx]`, `[FFxx:pppp]`)
ficam todos; letra acentuada não existe na fonte do texto (o app troca na
resposta da IA; no TSV, escreva sem acento); frase de espaço fixo cabe no
espaço; linha que passa da caixa é erro onde o app não quebra sozinho.

Lixo lido como texto: `not-text <ids>` (ou a IA responde `<NOT_TEXT>`). Fica
marcado mesmo extraindo de novo.

## 4. Texto desenhado (imagens)

```bash
romtranslate pictures jogo.rtsproj                        # folhas em imagens/
romtranslate read-pictures jogo.rtsproj <id> --preview    # folha de trechos
romtranslate read-pictures jogo.rtsproj <id> [--leitura leitura.json]
romtranslate pictures jogo.rtsproj --sem-texto <ids>      # conferidas sem japonês
```

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
- **Campos de espaço fixo** (nomes de inimigo, item, PSI): a tradução cabe no
  espaço do maior nome da tabela; sai abreviada.
- **Tradução em inglês ou abreviada** de rodadas antigas: procure no banco do
  projeto e refaça (no jogo medido, um lote inteiro do roteiro tinha voltado
  em inglês).
