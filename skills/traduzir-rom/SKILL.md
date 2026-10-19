---
name: traduzir-rom
description: Traduz um jogo retrô (ROM de GBA, DS, NES, SNES, PS1...) com o RomTranslate Studio quando o app não acha o texto sozinho — estuda o arquivo até escrever o perfil do jogo (perfil.toml + tabela de letras), traduz pelo CLI, confere no emulador sem janela e exporta o patch. Use para traduzir ROM, achar tabela de letras ou ponteiros, ou escrever perfil de jogo.
---

# Traduzir um jogo com o RomTranslate Studio

O app traduz, valida e regrava com segurança; o que ele não sabe sozinho é
**onde mora a fala** de cada jogo. Este roteiro leva do arquivo ao patch:
primeiro a extração automática; se ela não serve, a investigação do arquivo
até o **perfil** (`perfil.toml` + `tables/<id>.tbl`, guia em
`docs/PERFIL.md`), e daí o mesmo CLI de sempre.

Nomes usados abaixo:
- `rt` = o CLI do app. No repositório: `cargo build --release -p romtranslate-cli`,
  e o binário fica em `target/release/romtranslate`.
- `$F` = a pasta `ferramentas/` ao lado deste arquivo (Python 3; o desenho da
  fonte usa Pillow). Cada ferramenta tem `--help`; `python3 -I $F/teste.py`
  confere todas num arquivo sintético.
- **fala** = o texto que aparece na tela: diálogo, menu, nome de item.

## Regras

- O jogo é do usuário e fica com ele: o projeto, a cópia traduzida e as fotos
  moram numa pasta fora do repositório, e o app só escreve na cópia
  (`<projeto>/working/`). O repositório recebe código e documentação, nunca
  byte de jogo.
- Para outras pessoas vai só o **patch** (`<projeto>/exports/*.ips|.bps`), o
  `perfil.toml` e a tabela `.tbl`. A ROM e o `translations.csv` (que traz o
  texto original do jogo) ficam na máquina.
- O jogo é visto só pelo emulador sem janela (`tools/gba-tela`,
  `tools/nds-tela`) e pelas fotos que ele grava. A tela do usuário é dele:
  nada de clique, captura ou automação nela.
- Tradução pela IA do app gasta a cota do usuário: mostre o `estimate` e
  espere o OK antes do `translate`.
- Rode o Python sobre a ROM com `python3 -I` (o arquivo é dado, não código).

## 1. Criar o projeto

```bash
rt inspect jogo.nds
rt create jogo.nds ~/traducoes/jogo/jogo.rtsproj --source en-US --target pt-BR
```

Olhe no `inspect` o formato e a confiança. Erro comum: arquivo `.zip`/`.7z`
— extraia antes, numa pasta vazia só dele.

Pronto quando: `projeto criado:` com o formato certo.

## 2. Extração automática: serve?

```bash
rt extract jogo.rtsproj
rt export jogo.rtsproj frases.json
python3 -I -c 'import json,sys,collections as c; d=json.load(open(sys.argv[1])); print(len(d),"frases"); print(c.Counter(e["resourcePath"] for e in d).most_common(8)); [print(repr(e["sourceText"][:70])) for e in d[::max(1,len(d)//25)]]' frases.json
```

Pergunte ao usuário (ou ache num vídeo/manual que ele indicar) três falas do
começo do jogo — o menu e a primeira conversa — e procure-as no
`frases.json`. Sinais de **lixo**: dezenas de milhares de "frases" (num DS
foram 55 mil, nenhuma fala); pedaços em MAIÚSCULAS sem minúscula
(`CASTLE`, `2ESTORES` = texto com as letras deslocadas, lido como ASCII);
frases vindas de vídeo, som ou modelo 3D (`resourcePath`); sequências de
pontuação.

- As três falas estão lá, inteiras → pule para o passo 5 (medir).
- Não estão → passo 3.
- Jogo japonês de GBA: a extração automática costuma ler; o caminho próprio
  dele está em `docs/PROCESSO-GBA-JAPONES.md`.

## 3. Investigar o arquivo até o perfil

Anote cada achado (posição, bytes, evidência) num arquivo de notas na pasta
do projeto: é com ele que você escreve o perfil.

**3a. Onde procurar.** No DS, o texto costuma morar num overlay, no ARM9 ou
num arquivo de dados:

```bash
python3 -I $F/nds_arquivos.py jogo.nds                 # módulos + endereço de carga
python3 -I $F/nds_arquivos.py jogo.nds --arquivos      # o sistema de arquivos
```

Overlay marcado `COMPRIMIDO` não se lê direto (o perfil não descomprime).
No GBA o cartucho inteiro é um bloco só.

**3b. Tabela de letras.** Pegue uma palavra de uma fala conhecida, com 4+
letras, de preferência minúscula (palavra só em maiúscula costuma ser
imagem):

```bash
python3 -I $F/achar_palavra.py jogo.nds castle --tbl letras.tbl
```

Olhe `lido:` — a frase em volta tem que sair legível — e `parece ASCII com
-0x20` (aí a tabela gravada já tem todo o ASCII deslocado). Sem resultado:
outra palavra, `--bytes 2` (letra de 16 bits), ou o texto está comprimido ou
desenhado em imagem. Pronto quando a frase em volta da palavra sai inteira.

**3c. Cabeçalho, terminador, alinhamento.** Na linha `bytes:` de 3 ou 4
palavras de falas diferentes, veja o que vem antes do começo da frase
(cabeçalho, ex. `01 00`) e logo depois do fim (terminador, ex. `EA`, `00`,
`FF`), e se toda frase começa num múltiplo de 2 ou 4. Confira:

```bash
python3 -I $F/listar_frases.py jogo.nds --tbl letras.tbl --cabecalho 0100 \
    --terminador EA --alinhamento 4 --inicio 0x120000 --fim 0x138000
```

Pronto quando a lista mostra frases inteiras, uma atrás da outra, e as três
falas conhecidas aparecem.

**3d. Códigos.** O fim da lista mostra cada código fora da tabela
(`[E3]`) e quantas vezes o byte seguinte não é letra; `talvez leve
parametro` (ou uma letra solta grudada no código, como `[E3]0The`) pede
`--parametros E3=1` — sem isso o parâmetro vira letra e a IA pode trocá-lo.
Identifique também o código que **quebra a linha** (fica entre duas linhas da
mesma caixa → `quebra`) e o que **espera o botão** no fim de uma página
(→ `pagina`). Pronto quando nenhuma frase tem letra solta grudada em código.

**3e. Ponteiros.** Ponteiro = palavra de 32 bits com
`memoria + (posição da frase − origem)`. GBA: `memoria 0x08000000`,
`origem 0`. DS: o endereço de carga e a posição do módulo onde a frase
mora:

```bash
python3 -I $F/nds_arquivos.py jogo.nds --onde 0x121004        # dá memoria e origem
python3 -I $F/achar_ponteiros.py jogo.nds --memoria 0x0219E3E0 --origem 0x120000 0x121004 0x121040
python3 -I $F/achar_ponteiros.py jogo.nds --memoria 0x0219E3E0 --origem 0x120000 --alvo 0x120000 0x138000
```

A posição é a do **cabeçalho** da frase. Sem ponteiro: tente a posição logo
depois do cabeçalho (o jogo pode contar do texto; aí o perfil leva
`cabecalho = ""`). As tabelas do `--alvo` viram o `procurar` do perfil.
Pronto quando as três falas conhecidas têm ponteiro. Se nenhuma tem, o jogo
usa outro tipo de ponteiro (16 bits, como no NES e no SNES, relativo, montado
no código), que o perfil ainda não cobre. Diga isso ao usuário; o que dá pra
fazer é traduzir no espaço da original, sem crescer:
`rt scan jogo.rtsproj --tbl letras.tbl --region 0x120000..0x138000` e
siga do passo 5.

**3f. Fonte e acentos.** Ache a fonte (DS: `--arquivos | grep -i font`;
ou a região de desenhos repetidos) e desenhe-a:

```bash
python3 -I $F/desenhar_fonte.py jogo.nds --inicio 0x3A0000 --largura 8,12,16 --saida fonte.png
```

Abra os PNGs: a largura certa mostra letras inteiras. Se a fonte tem
à á â ã ç é ê í ó ô õ ú, ponha cada código na tabela (`6B=À`), conferido
no desenho, um por um. Sem acento na fonte, o app grava a letra sem acento
— avise o usuário.

**3g. Espaço.** O perfil regrava o banco dentro do espaço que ele já ocupa
(o arquivo não cresce). No DS, veja no `nds_arquivos.py` se o módulo seguinte
carrega logo depois deste na memória: se sim, não há sobra e nomes curtos
vão sair abreviados. Diga isso ao usuário antes de traduzir.

**3h. Escrever o perfil.** Na pasta do projeto:

```toml
# <projeto>/perfil.toml — campos em docs/PERFIL.md
[[texto]]
nome = "overlay 0"
tabela = "letras"            # <projeto>/tables/letras.tbl
inicio = 0x120000
fim = 0x138000
cabecalho = "0100"
terminador = "EA"
alinhamento = 4
quebra = "E6"
pagina = "E5"
parametros = { E1 = 2, E3 = 1 }

[texto.ponteiros]
memoria = 0x0219E3E0
origem = 0x120000
procurar = [[0x120000, 0x140000]]
```

Copie `letras.tbl` para `<projeto>/tables/letras.tbl`. Vários bancos =
vários `[[texto]]`.

## 4. Extrair com o perfil

```bash
rt extract jogo.rtsproj
rt export jogo.rtsproj frases.json
```

Olhe `gravadas: N, descartadas: M`: N perto das frases com ponteiro que você
listou; M é dado que abre igual a uma frase. Leia uma amostra do
`frases.json` (o comando do passo 2): frases inteiras, códigos como `[E6]` e
`[E3:10]`. A tabela ganha no fim uma seção gerada — não a edite. Erros
comuns: `gravadas: 0` (memoria/origem ou `procurar` errados: volte ao 3e);
`perfil.toml invalido` (TOML); tabela não achada (nome em `tabela`).

Pronto quando as três falas conhecidas estão no projeto, inteiras.

## 5. Medir

```bash
rt estimate jogo.rtsproj
```

Chamadas e tokens da rodada. Num jogo de DS medido, o gasto real foi
várias vezes o estimado (reperguntas e instruções não entram na conta):
diga isso junto com os números e espere o OK do usuário.

## 6. Traduzir

- Pela IA das configurações do app (com o OK): `rt translate jogo.rtsproj`.
  Parou no meio (cota, cancelamento)? Rodar de novo manda só o que falta.
- Por fora, sem a IA do app (você mesmo traduz, com os tokens desta
  sessão — combine o volume com o usuário): `rt estimate jogo.rtsproj --pedidos lotes.json`
  grava os lotes exatos (instruções em `system`, frases em `items` com `id`,
  `text`, `maxChars`, `maxBytes`, `maxLine`). Traduza seguindo as instruções
  — todo código `[..]` mantido, dentro de `maxChars`, acento só se a tabela
  tem — e grave um TSV `id<TAB>tradução`, uma por linha:
  `rt apply jogo.rtsproj traducao.tsv`.

## 7. Validar e corrigir

```bash
rt validate jogo.rtsproj
```

Cada erro diz a frase e o motivo: `byte_overflow` (passou do espaço, ou
linhas a mais na página da caixa), `placeholder_mismatch` (código `[..]`
faltando ou trocado), `unencodable` (letra que a tabela não escreve). Corrija:
- de novo pela IA, só as com erro: `rt translate jogo.rtsproj --redo-errors`
  (gasta cota; `rt estimate jogo.rtsproj --redo-errors` mede antes);
- à mão: TSV só com as linhas corrigidas, `rt apply`;
- dado lido como texto: `rt not-text jogo.rtsproj <ids>`.

Pronto quando: `erros: 0`.

## 8. Reinserir

```bash
rt reinsert jogo.rtsproj --policy block
```

Olhe `aplicadas`, `sem espaco no arquivo: N` e `verificacao: ok`. N > 0:
as frases que cresceram não acharam lugar no banco — encurte os nomes
curtos primeiro (o teto deles conta com uma sobra que nem sempre fica
contígua) e reinsira. `--policy skip` grava o resto e deixa as com erro no
original. O `audit` só conta japonês; num jogo em outra língua, veja o que
sobrou listando a cópia:

```bash
python3 -I $F/listar_frases.py jogo.rtsproj/working/jogo.nds --tbl jogo.rtsproj/tables/letras.tbl \
    --cabecalho 0100 --terminador EA --alinhamento 4 --parametros E3=1 --inicio 0x120000 --fim 0x138000
```

Pronto quando: `verificacao: ok`, `sem espaco no arquivo: 0` e a lista da
cópia em português.

## 9. Ver rodando

```bash
tools/nds-tela/montar.sh            # uma vez por máquina (GBA: tools/gba-tela)
tools/nds-tela/rodar.sh jogo.rtsproj/working/jogo.nds fotos 600 START 120 shot=titulo A 300 shot=fala
```

Precisa de `cmake`, `git` e `libpng`: se faltar, peça ao usuário para
instalar. O roteiro de teclas e fotos está no README de cada ferramenta.
Leia as fotos e percorra no mínimo: título, menu, primeira fala, uma fala com
acento. Texto cortado, letra errada ou caixa estourada → volte ao passo 7.

Pronto quando: as telas vistas estão em português, com acento certo.

## 10. Patch

```bash
rt patch jogo.rtsproj            # IPS se couber, senão BPS
```

O patch sai em `jogo.rtsproj/exports/`. Entregue ao usuário: o patch, o
`perfil.toml` e a tabela, mais o que ficou de fora (imagens com texto, nomes
abreviados, telas não vistas).
