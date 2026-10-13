# RomTranslate Studio — detalhes técnicos

> O que cada parte faz, a matriz completa de compatibilidade, os limites medidos, a CLI e a arquitetura.
> A apresentação do projeto está no [README](../README.pt-BR.md) ([English](../README.md)).

Ferramenta desktop open source para extrair, traduzir, revisar e reaplicar textos em
jogos compatíveis, com IA local (Ollama) ou APIs configuráveis, gerando **patches** (IPS/BPS)
em vez de cópias modificadas.

**Status: alpha.** O pipeline completo funciona de ponta a ponta:
detecção → extração → tradução com IA → validação/revisão → reinserção em
cópia de trabalho → **patch IPS ou BPS** com manifest.

![Tela de projeto do RomTranslate Studio: extração de strings de um ISO de PS2 com traduções, statuses e validação](assets/screenshot-full.png)

*Projeto fictício de demonstração — nenhum dado de jogo real.*

## Baixar

Binários prontos saem em
[Releases](https://github.com/crimsondawnacademy-design/romtranslate-studio/releases):
`.dmg` para macOS (Apple Silicon e Intel), `-setup.exe` ou `.msi` para Windows,
`.deb`, `.rpm` ou `.AppImage` para Linux — prefira `.deb`/`.rpm` (~6 MB); o
AppImage roda sem instalar, mas pesa ~80 MB porque embute as dependências.

Os binários **não são assinados** — projeto pessoal, e certificado de
desenvolvedor é pago. Na primeira execução o sistema reclama; é esperado:

- **macOS** ("o app está danificado"): depois de arrastar pra Aplicativos, rode
  no Terminal `xattr -cr "/Applications/RomTranslate Studio.app"`
- **Windows** (SmartScreen): "Mais informações" → "Executar assim mesmo"
- **Linux**: `chmod +x RomTranslate*.AppImage`

Prefere compilar do código? Veja [Rodando](#rodando).

## O que funciona hoje

- **Detecção** de plataforma com confiança e evidências em 11 formatos (GBA,
  NES, SNES, NDS, GameCube, Wii, Wii U, PS1, PS2, PSP e a fixture sintética
  RTSF);
- **Extração**: scanner genérico (ASCII, UTF-8, UTF-16 LE/BE, Shift-JIS,
  tabelas `.tbl`)
  para descoberta + extração estruturada com limites reais nos adapters que a
  suportam. Sequência de bytes imprimíveis que não parece texto (gráfico,
  código, tabela) é descartada antes do teto de 100 mil strings, e a tela diz
  quantas foram;
- **Busca relativa**: jogo de cartucho quase nunca usa ASCII. Digite uma
  palavra que você sabe que aparece no jogo e o app descobre onde o alfabeto
  dele começa e monta o `.tbl` (letras e espaço; números e pontuação você
  completa no arquivo). A tradução é regravada no jogo pela mesma tabela;
- **Raio-X**: mostra o que o app *não* lê no arquivo — blocos comprimidos em
  LZ77 (o do GBA e do DS) e quantas frases estão presas neles, quanto do
  arquivo é denso (comprimido ou cifrado), e quantas das strings extraídas
  têm ponteiro em tabela e podem crescer;
- **Tradução por IA**: Ollama (local, padrão), qualquer endpoint
  OpenAI-compatible, o Claude pela API da Anthropic com a sua chave de API,
  ou a sua assinatura do ChatGPT pelo Codex CLI (sem chave de API); batching,
  retry, cancelamento, progresso. Com chave paga, o app **calcula o gasto**
  antes (chamadas, tokens e, com o preço do modelo, o valor) e mostra o que o
  provedor cobrou depois; **translation
  memory** e **glossário** por projeto (SQLite); tradução remota exige opt-in
  explícito de privacidade. Texto ASCII e texto de tabela vão com a lista dos
  caracteres que o jogo escreve, e nome interno do jogo (identificador) fica
  fora da fila;
- **Validação e revisão**: placeholders/control codes/tags, limite de bytes no
  encoding destino, editor com filtros e statuses; erros bloqueiam a reinserção;
- **Reinserção** em working copy verificada — o arquivo original **nunca** é
  modificado;
- **Patch IPS ou BPS** + `manifest.json` + CSV de traduções, com round-trip
  interno conferido antes de exportar. Compatível com Lunar IPS, Floating IPS,
  beat, RetroArch e afins; o BPS valida por CRC que o patch é para o arquivo
  certo e suporta qualquer tamanho.

## Matriz de compatibilidade

Jogos usam engines, compressões e tabelas diferentes — **não prometemos suporte
universal**. O que cada plataforma tem hoje:

| Plataforma | Detecção | Scan genérico | Extração estruturada | Reinserção | Patch |
|---|---|---|---|---|---|
| Fixture RTSF (demo) | ✅ | ✅ | ✅ completa | ✅ com relocação + ponteiros | ✅ |
| Game Boy Advance | ✅ | ✅ | ⚠️ experimental (detecta tabelas de ponteiros) | ⚠️ experimental (in-place + **relocação** de strings com ponteiro em tabela) | ✅ (BPS se o ROM de 16 MiB crescer) |
| Nintendo DS | ✅ (CRC do header) | ✅ | ⚠️ experimental (por arquivo do filesystem, ASCII + UTF-16) | ⚠️ experimental (in-place + **relocação**: o arquivo cresce e a FAT é reapontada) | ✅ (BPS acima de 16 MiB) |
| NES | ✅ | ✅ | ⚠️ experimental (in-place; jogos com tabela própria: use `.tbl`) | ⚠️ experimental (in-place; texto lido por `.tbl` volta pela tabela) | ✅ |
| Super Nintendo | ✅ | ✅ | ⚠️ experimental (in-place) | ⚠️ experimental (in-place, checksum interno recalculado) | ✅ |
| GameCube | ✅ (magic + FST) | — | ⚠️ experimental (por arquivo do filesystem FST) | ⚠️ experimental (in-place) | ✅ (BPS p/ ISO real) |
| Wii | ✅ (ISO e WBFS) | — | — | — | — |
| Wii U | ✅ (WUX e RPX/RPL) | — | — | — | — |
| PlayStation (PS1) | ✅ (ISO 9660 + SYSTEM.CNF) | ✅ | ⚠️ experimental (por arquivo; BIN raw por setor) | ⚠️ experimental (in-place + **relocação** na sobra do setor final; BIN raw com EDC/ECC regenerado)* | ✅ |
| PlayStation 2 | ✅ (BOOT2 no SYSTEM.CNF) | ✅ | ⚠️ experimental (por arquivo; DVD inteiro via mmap, sem teto de RAM) | ⚠️ experimental (in-place; >2 GiB em streaming) | ✅ (BPS, round-trip streaming) |
| PSP | ✅ (UMD + PARAM.SFO com título) | ✅ | ⚠️ experimental (por arquivo) | ⚠️ experimental (in-place) | ✅ (BPS) |

\* BIN raw de PS1 (2352 bytes/setor): a reinserção regenera o EDC/ECC
(ECMA-130) de cada setor alterado, e o verify confere o EDC da imagem inteira.
Dump multi-track? Abra o **`.cue`**: o app resolve o track de dados sozinho
(layout Redump — dados como primeiro track do FILE) e os tracks de áudio
ficam intactos — o verify ignora setores sem sync. A working copy e o patch
são do BIN de dados; o `.cue` original continua valendo pra ele.

O export escolhe o formato sozinho: **IPS** quando cabe (máxima
compatibilidade) e **BPS** para arquivos maiores ou que encolhem — o BPS ainda
valida por CRC-32 que o patch está sendo aplicado no arquivo certo. Partições
de disco Wii são cifradas: extração exigiria keys, que este projeto não inclui.

**Lixo que parece texto**: num cartucho, a maior parte dos bytes imprimíveis
não é texto — é gráfico, código e tabela. Num jogo real de GBA de 16 MiB
medido em 12/10/2026 (só contagens; nenhum byte de jogo entra neste
repositório), o scanner ASCII achou 187.162 sequências de 4 ou mais
caracteres. Com o filtro ficam 17.936 e 169.226 são descartadas. Antes o app
parava nas 20 mil primeiras, das quais só 3.124 passam no filtro; as outras
14.812 que ficam estavam depois do corte, fora da lista.

A regra (`looks_like_text`, em `crates/core/src/scan.rs`) olha a proporção de
letras, a variedade, a caixa de cada palavra e se há vogais; palavra curta de
menu ("SAVE", "Yes", "HP") passa. String com ponteiro em tabela nunca é
descartada. No **Escanear** o filtro é a caixa "Só o que parece texto", ligada
por padrão: desmarque para ver tudo. A **Extração estruturada** filtra sempre,
porque o que ela devolve é regravado no jogo — lixo ali é tradução gravada por
cima de gráfico ou de código. Limites:

- é heurística, e erra: perde "McCoy" (caixa no meio da palavra), "Lv.99" e
  "HP: 12/30" (símbolo demais) e "Nooooo!" (repetição demais). Uma string real
  que a regra descarta aparece no Escanear sem filtro, mas esse caminho não
  regrava no jogo;
- foi calibrada num jogo só, em inglês;
- projeto criado antes do filtro: a **Extração estruturada** tira dele as
  strings antigas que não parecem texto e diz na tela quantas saíram. Só sai o
  que não tem tradução, não tem ponteiro em tabela, não foi lido por tabela de
  caracteres (`.tbl`) e não veio na extração que acabou de rodar. Lixo que já
  recebeu tradução (da IA inclusive) fica na lista, e o **Escanear** nunca
  apaga nada;
- no Nintendo DS, a leitura UTF-16 falsa de um trecho binário passa pela regra
  (para ela, kanji é letra).

**Texto fora do alcance do scanner**: extração vazia não prova que o jogo tem
pouco texto. Se ele usa tabela de caracteres própria, a **busca relativa**
monta o `.tbl`, e o texto lido por tabela é **regravado no jogo** por ela. O
scan guarda uma cópia da tabela em `tables/` dentro do projeto; é essa cópia
que a validação e a reinserção usam, e é nela que você completa números e
pontuação. Vale a regra do in-place, medida em bytes da tabela: a tradução
ocupa o espaço da original ou menos. A tradução por IA recebe a lista dos
caracteres que a tabela escreve e a instrução de não usar nenhum outro. Se
mesmo assim vier uma letra com acento que a tabela não tem, ela é gravada
como a letra sem acento (`ação` vira `acao`) quando essa letra está na
tabela, e a frase ganha um aviso de validação com o que a IA respondeu e o
que foi gravado — tirar o acento pode mudar o sentido (`avô` e `avó` viram
`avo`), então confira. Para ter o acento no jogo, a linha dele precisa estar
no `.tbl` (e o jogo precisa ter o desenho da letra; o app não edita fonte).
O que continuar fora da tabela (uma pontuação que falta completar, um acento
cuja letra base a tabela não tem) é erro que diz qual caractere e em que
posição — nunca um byte inventado. Limites: isso está provado de ponta a
ponta numa fixture sintética de NES com um provider de mentira, não em jogo
real; as outras plataformas in-place usam o mesmo caminho, mas não têm teste
com tabela. Com modelo de verdade há duas medições, nas 7 frases da fixture.
Num modelo local pequeno (`llama3.2:3b`): nenhuma pontuação fora da tabela, e
3 ou 4 das 7 respostas ainda vieram com acento e foram gravadas sem ele (nas
6 rodadas em que isso foi contado). No ChatGPT pelo Codex CLI (modelo padrão
do Codex em 12/10/2026, `gpt-6-astra`): nenhum acento e nenhum caractere fora
da tabela, em 7 rodadas. A troca
cobre letra latina acentuada num caractere só (`á`, `ç`, `É`) e aspas,
travessão e reticências tipográficos (quando a tabela tem o sinal simples);
tradução escrita à mão não é mexida.

**Tamanho do texto de tabela**: ele é sempre gravado no lugar, então o
espaço da original é o teto. A tradução por IA recebe esse teto em cada
frase, em caracteres (`maxChars`), com a instrução de que é limite duro. O
número é contado pela entrada mais cara da tabela: numa tabela de 1 byte por
caractere é o próprio espaço; se algum caractere custa 2 bytes, é a metade —
nunca mais do que cabe, às vezes menos. Código de controle ou placeholder
que a tradução tem de copiar do original (`[FIM]`, `{0}`) entra pelo que
custa em bytes, não pelos caracteres que o modelo conta: se `[FIM]` é 1
byte, `HELLO[FIM]` (6 bytes) vai com teto de 10 caracteres, não 6. Dupla de
letras num byte só (`th`) continua sem aumentar o teto. A memória de
tradução passa pela mesma conta: tradução guardada que não cabe no espaço
da frase (veio de outro projeto, ou de uma resposta antiga que estourou)
não é usada, e a frase vai para o provider com o teto. Quem decide se cabe
é a validação,
que mede em bytes da tabela: tradução maior que o espaço é **erro** na frase
(antes era aviso, e a recusa só aparecia na reinserção), com os dois
tamanhos. O app **não corta** texto: a tradução fica inteira no projeto até
você encurtar, e não pergunta de novo ao modelo as frases que passaram.

O que foi medido, nas 7 frases da fixture (inglês para pt-BR, tabela de 1
byte por caractere, só letras e espaço):

| Modelo | Pedido | Rodadas | Maiores que o espaço | Caracteres a mais, somando as 7 |
|---|---|---|---|---|
| ChatGPT pelo Codex CLI (`gpt-6-astra`) | com o limite | 4 | 0 de 7 nas 4 | 0 |
| ChatGPT pelo Codex CLI (`gpt-6-astra`) | sem o limite | 3 | 7 de 7 nas 3 | 26 a 30 |
| `llama3.2:3b` (Ollama local) | com o limite | 7 | 5 a 7 de 7 | 17 a 43 |
| `llama3.2:3b` (Ollama local) | sem o limite | 3 | 7 de 7 nas 3 | 30 a 42 |

No modelo capaz é o limite que faz caber. No modelo de 3B ele não basta: a
diferença entre ter e não ter o limite fica dentro da variação de uma rodada
pra outra — ali conte com o erro de validação e com encurtar à mão. Caber
não é traduzir bem: `CONTINUE` (8 de espaço) voltou do Codex sem traduzir nas
4 rodadas, porque `CONTINUAR` tem 9. Não medido: jogo real, diálogo longo, tabela com
caractere de 2 bytes, outro par de idiomas, outro modelo. Limite conhecido:
string de largura fixa numa tabela sem espaço precisa do tamanho exato, e o
pedido só diz o máximo. E a sobra de
uma tradução mais curta vira `0x00` quando o byte depois da string original
é `0x00` (senão é preenchida com o espaço da tabela) — jogo com outro
terminador pede conferência no emulador.

**Disco grande**: num disco, vídeo, áudio e arquivo comprimido dão centenas
de "palavras" de 4 letras por MiB que passam no filtro de texto. A extração
dos discos (PS2, PSP, GameCube) julga cada bloco de 64 KiB do arquivo: fica o
bloco que tem pelo menos uma frase ou meio por cento dos bytes em strings com
cara de texto, o resto é descartado e contado. O scanner ASCII lê uma
sequência por vez, então a memória não cresce com o tamanho do disco. Medido
em 13/10/2026 num disco de teste com 64 MiB de dado aleatório: de 1,1 GiB de
RAM e 19.899 strings de lixo para 76 MiB e nenhuma, em metade do tempo.
Calibrado em ruído, não em disco de jogo: executável com rótulo curto e
esparso no meio de código perde esses rótulos. No PS1 (que precisa das
sequências cruas para achar ponteiros) o corte por bloco não entra.

**Jogo em japonês (Shift-JIS)**: a extração estruturada do Game Boy Advance,
do DS, do GameCube e do PS1, PS2 e PSP lê, além do ASCII, o texto japonês em
Shift-JIS, e cada faixa de bytes sai numa leitura só. Dado binário lido como
Shift-JIS vira kanji o tempo todo e quase nunca vira kana, então a regra pede
kana: dois diferentes, em pelo menos um quarto da escrita japonesa da frase.
Palavra só de kanji ("装備") fica de fora. Medido em 13/10/2026, só em
contagens: 16 MiB de ruído não dão nenhum falso positivo (davam 40 com a
regra frouxa), e num jogo ocidental de GBA de 16 MiB passam 51 sequências,
22 delas nomes de recurso deixados pelos desenvolvedores; todas caem fora das
regiões de texto e nenhuma frase do jogo se perde. A tradução volta em
Shift-JIS: letras comuns num byte cada, sem acento (a tabela japonesa não tem
letra acentuada; a IA é avisada e o app troca o que vier acentuado, com
aviso). A frase sem ponteiro em tabela precisa caber no espaço da original e
a IA recebe esse teto já no primeiro pedido. O scanner da tela também tem a
opção `shift_jis`, que lê tudo sem filtrar. **Nenhum jogo japonês real passou
por isto.** Jogo de cartucho que usa tabela de caracteres própria (quase todo
NES e SNES) continua pedindo a tabela.

**Jogo em japonês com códigos de 16 bits (ordem JIS)**: nem todo jogo
japonês usa Shift-JIS. O primeiro jogo japonês real medido (um RPG de Game
Boy Advance de 32 MiB, em 13/10/2026, só em contagens) guarda as falas em
códigos de 16 bits que são a posição do caractere na tabela JIS X 0208
(`(linha - 1) * 94 + (coluna - 1)`, em little-endian), com os comandos do
jogo de `0xFF00` para cima, `0xFFFF` fechando a mensagem e um código próprio
fazendo o papel do espaço entre palavras. O scanner Shift-JIS acha nele 5
frases, todas mensagens de erro dos programadores. A extração do Game Boy
Advance agora reconhece esse formato sozinha (`crates/core/src/kuten.rs`):
se o arquivo tem pelo menos 100 frases lidas assim, ela extrai as mensagens
inteiras (os comandos do meio viram `[FFxx]` no texto, e a tradução tem que
mantê-los), descobre o código do espaço (o código fora da tabela que mais
aparece entre dois kana) e grava no projeto a tabela `tables/jis16.tbl`,
por onde a validação, a tradução e a reinserção passam como em qualquer
texto de tabela. Texto de jogo vem em fila (o roteiro, a tabela de nomes, a
de títulos de música): só vale a sequência que tem pelo menos duas vizinhas
a até 128 bytes, ou a que é longa sozinha, e que começa logo depois do fim
de outra mensagem, de um comando ou de preenchimento.

O leitor faz duas passadas, e na primeira aprende o jeito daquele jogo:

- **comando com parâmetro**: o comando que quase nunca é seguido de algo que
  continue um texto leva um número depois dele, e o par vira uma peça só no
  texto (`[FF08:0003]`). Sem isso o número era lido como letra, e traduzir a
  frase trocava o parâmetro do comando;
- **marca de fala e número de cabeçalho**: as figuras com que as falas abrem
  ("◆", "◇"); um valor baixo entre o fim de uma mensagem e a marca é
  cabeçalho, e fica fora da frase;
- **desenho próprio do jogo**: código que a tabela padrão deixa vazio (kanji
  e ícone da fonte do jogo) fica no texto como `[xxxx]`, e a linha
  correspondente do `jis16.tbl` pode ser trocada pelo caractere certo.

No jogo medido: 12.296 mensagens na fila (876 KiB de texto), que cobrem
96,6% do kana do cartucho; o que fica de fora são regiões de dado. 7 comandos
com parâmetro e 2 marcas de fala descobertos; 414 falas tinham número de
cabeçalho. Sobram umas 70 sequências (0,6%) que são dado com cara de texto,
em tabelas da fonte e do teclado de nomes. As 6.669 strings de 8 bits (ASCII
e Shift-JIS) saem da fila, porque num jogo assim o que o scanner de 8 bits lê
é quase tudo dado. No jogo ocidental de GBA e em ruído, nada é reconhecido.
Os kanji desse jogo não seguem a ordem da tabela (ele tem uma lista própria):
os que caem em código vazio aparecem como `[xxxx]`, os outros saem como um
kanji qualquer. São poucos (o jogo escreve quase tudo em kana). **Gravar de volta** é onde aperta: a tabela japonesa não tem a
letra "a", tem a de largura inteira "ａ", então cada letra da tradução
ocupa 2 bytes e é gravada nessa forma (`TblTable::encode`), sem acento; e
como a frase não muda de lugar, a tradução pode ter no máximo o mesmo número
de letras que o original tem de caracteres, o que do japonês para o
português quase nunca basta. No texto do próprio jogo aparecem 25 das 26
maiúsculas de largura inteira e só 13 das 26 minúsculas: não há prova de que
a fonte do jogo tenha todas. **Ninguém viu uma tradução gravada assim rodando
num emulador.** O que falta para esse jogo virar jogável em português é o
que falta em geral: frase que muda de lugar (tabela de ponteiros relativos)
e fonte (letra estreita e acentuada).

**Tradução que cresce: banco de mensagens em blocos**
(`crates/core/src/adapters/bank.rs`). O roteiro do jogo japonês medido fica
num banco com esta forma: um número e, depois dele, uma fila de posições de
32 bits relativas ao começo do banco, aos pares (a tabela de um bloco e o
texto dele); a tabela de cada bloco é uma fila de posições de 16 bits
relativas ao bloco de texto, uma por mensagem; cada mensagem fecha com
`0xFFFF`. O app reconhece o banco pela própria estrutura (centenas de
posições que têm que cair no lugar certo), sem saber que jogo é. Com isso:

- a extração usa os começos de mensagem que a tabela aponta, e a mensagem
  de banco deixa de ter teto de bytes;
- na reinserção o bloco é remontado com as mensagens em fila e a tabela
  refeita. Se o bloco novo cabe onde o antigo estava, fica lá; senão vai
  para o espaço livre do arquivo (trecho de 64 KiB ou mais de `0xFF`, e o
  lugar dos blocos que saíram) e o diretório é reapontado. Bloco que não
  acha lugar, ou que passa de 64 KiB, fica como no original, e a contagem
  aparece na tela;
- o limite passa a ser a **caixa de texto**: a largura de linha (trecho
  entre dois comandos) que 999 em cada mil linhas do banco respeitam. A IA
  recebe essa largura e a validação avisa a linha que passa dela.

Medido no jogo real, só em contagens: um banco de 563 blocos e 8.120
mensagens, com 85% do texto do cartucho; caixa de 20 caracteres por linha
(99,96% das 31 mil linhas); 359 KiB de preenchimento livre. Num teste de
esforço, as 7.338 mensagens do banco foram trocadas por texto 1,5 vez maior:
todas entraram, 414 dos 481 blocos com tabela mudaram de lugar, 336 dos 359
KiB livres foram usados, e a leitura pelo diretório, mensagem a mensagem,
deu os mesmos comandos e parâmetros do original nas 8.120. **Ninguém viu o
jogo rodando com blocos fora do lugar.** Os outros bancos de texto desse
jogo (menus, batalha, nomes: 15% do texto) têm outra estrutura e continuam
com a tradução presa ao espaço da original.

**Texto ASCII traduzido para idioma com acento**: num jogo que guarda o
texto em ASCII (é o que a extração estruturada lê; no DS, também UTF-16),
letra acentuada não existe. O lote de strings ASCII vai para a IA com a lista
do ASCII imprimível e a instrução de que não há letra acentuada — o mesmo
mecanismo do texto de tabela. Se a resposta vier mesmo assim com acento, ou
com aspas, travessão e reticências tipográficos (`“ ” ‘ ’ — – …`), o app
grava a letra sem acento e o sinal simples (`" ' - ...`) e a frase ganha um
aviso de validação com o que a IA respondeu e o que foi gravado. O que não
tem equivalente (`º`, `ß`) continua erro, e a mensagem diz qual caractere e
em que posição. Texto UTF-8 e UTF-16 escreve acento e não é mexido: vai em
lote separado, sem a lista. A memória de tradução passa pela mesma rede, e
tradução guardada que nem assim cabe em ASCII não é usada.

Medido em 12/10/2026 no mesmo jogo real de GBA, com 30 falas espalhadas pelo
arquivo e o ChatGPT pelo Codex CLI (`gpt-6-astra`), 3 rodadas de cada pedido
(só contagens; o texto do jogo não entra neste repositório):

| Pedido | Respostas com caractere fora de ASCII | Passam na validação como a IA respondeu | Passam como o app grava | Maiores que o campo |
|---|---|---|---|---|
| sem a lista (o de antes) | 17, 18 e 19 de 30 | 12, 10 e 10 de 30 | 29, 28 e 28 de 30 | 1, 2 e 2 |
| com a lista (o de hoje) | 0 de 30 nas 3 | 26, 27 e 27 de 30 | 26, 27 e 27 de 30 | 4, 3 e 3 |

Com a lista o modelo não usou acento em nenhuma das 90 respostas, e a troca
não precisou agir; sem ela, a troca sozinha já tira o acento do caminho. O
que sobra barrado é tamanho: tradução maior que o campo de uma linha da
caixa de texto (erro que manda encurtar; nada é cortado). Com a lista houve
mais dessas (10 em 90, contra 5 em 90), numa amostra pequena demais para
dizer que a causa é a lista. Na primeira rodada com a lista, as 26 válidas
foram reinseridas: arquivo do mesmo tamanho, 698 bytes alterados, nenhum
fora de string traduzida, verificação ok, patch IPS de 885 bytes. Limites:
um jogo, um par de idiomas, um modelo; tirar o acento pode mudar o sentido
(`avô` e `avó` viram `avo`); para ter acento no jogo o texto precisa ser
lido por uma tabela `.tbl` que tenha a letra, e o jogo precisa ter o desenho
dela.

**Nomes internos**: jogo guarda, ao lado do texto, nomes que nunca aparecem
na tela (sala, script, recurso). A extração marca como **nome interno** a
string feita só de maiúsculas, dígitos e sublinhado, sem espaço, com pelo
menos um sublinhado ou um dígito (`FASE4_31_01`, `VILA_SET_1`, `SALA12`).
Palavra de menu (`START`) e frase em maiúsculas (`NEW GAME`) continuam
texto. A string marcada fica na lista — ela pode ter ponteiro em tabela, e o
Raio-X conta com ela —, mas não vai para a IA e, sem tradução, não é
regravada. A tela de tradução diz quantas ficaram de fora, o filtro "nomes
internos" mostra quais, e o botão "É texto do jogo" desfaz a marca de uma.
No jogo medido, 2.326 das 17.936 strings saem da fila (sobram 15.610): 1.596
com sublinhado, das quais 1.564 têm ponteiro em tabela, e 730 só com dígito
colado, que ali são gráfico que passou no filtro de texto. Das 1.590 strings
com ponteiro em tabela, 1.565 são marcadas; as 25 que ficam são texto de
menu (21) e lixo (4). Limites: é heurística — `PLAYER1`, `STAGE1` e `1UP`
são texto e batem na regra; a marca é refeita a cada extração enquanto a
string não tem tradução (desfazer e extrair de novo marca outra vez); nome
interno que já recebeu tradução num projeto antigo fica como está.

Se o texto está comprimido, o **Raio-X** mostra quanto. O app só reconhece
o LZ77 tipo 0x10. No Game Boy Advance a extração estruturada lê as frases de
dentro dos blocos que são texto (pelo menos uma frase e um terço do bloco em
strings com cara de texto) e a reinserção recomprime o bloco: ele volta para
o lugar se couber nos bytes que ocupava, vai para o fim do ROM se o jogo
tiver uma tabela de ponteiros para ele, e fica como no original (com a
contagem na tela) se não tiver nenhum dos dois. Dentro do bloco a tradução
ocupa o espaço da frase original. Conferido só com arquivo sintético. No DS
e nos outros consoles o texto comprimido ainda é só contado.

**"In-place"**: cada string traduzida ocupa o espaço da original (mesmo
tamanho ou menor) — cobre menus e textos curtos de muitos jogos. O validador
avisa o que não cabe antes de qualquer escrita.

**Relocação de ponteiros**: quando a tradução não cabe no espaço da
original, o texto novo vai pra outro lugar e os ponteiros pra ele são
reapontados. Só vale pra string referenciada por uma **tabela** de ponteiros
(vários seguidos, cada um apontando pro início de uma string). O app **não**
caça cada ocorrência do endereço no arquivo: uma palavra de código ou de dado
pode ter o mesmo valor de um ponteiro por coincidência, e trocá-la
corromperia o jogo em silêncio. Onde o texto novo vai depende da plataforma:

- **GBA**: fim do ROM (o cartucho é lido direto pela CPU). Ponteiro de ROM
  quase nunca aparece por acaso, então 2 seguidos já contam como tabela.
- **Nintendo DS**: fim do arquivo do filesystem; o arquivo inteiro vai pro
  fim do ROM e a FAT passa a apontar pra ele. O ponteiro ali é um
  deslocamento pequeno dentro do arquivo, de 32 ou 16 bits — e número pequeno
  aparece à toa em dado binário. Por isso a tabela precisa estar em **ordem
  crescente**, com 3 seguidos (32 bits) ou 4 (16 bits); ponteiro de 16 bits
  só alcança os primeiros 64 KB do arquivo.
- **PS1**: sobra do último setor do arquivo — o hardware lê setores
  inteiros, então esse espaço sempre chega na RAM. É pouco (menos de 2 KB por
  arquivo, dividido entre as strings realocadas); o editor mostra o teto de
  cada uma. Mesmas tabelas de 32 ou 16 bits do DS.

**Fica de fora, de propósito**: o binário ARM9 do DS e o executável do PS1.
Os dois são copiados pra RAM, e logo depois deles vêm variáveis que o jogo
zera ao ligar e o heap — texto anexado ali seria apagado ou sobrescrito. PS2
e PSP também ficam só in-place: eles leem arquivos byte a byte, e a sobra do
setor não chega garantida na memória.

Em qualquer plataforma, o texto antigo continua no lugar: uma referência que
a detecção não viu mostra o original em vez de lixo. E relocar resolve o
espaço **na imagem**, não **na tela**: o validador avisa cada string
realocada pra você conferir no emulador se ela cabe na caixa de texto.
Projetos antigos: rode a extração estruturada de novo pra detectar as
tabelas (as traduções são preservadas).

## Rodando

Pré-requisitos: [Rust](https://rustup.rs) estável, Node 20+, [pnpm](https://pnpm.io),
e as [dependências do Tauri 2](https://tauri.app/start/prerequisites/) do seu SO.
Para tradução local, [Ollama](https://ollama.com) com um modelo puxado
(ex.: `ollama pull llama3.2:3b`).

Para traduzir pela assinatura do ChatGPT, instale o
[Codex CLI](https://github.com/openai/codex), rode `codex login` uma vez e
escolha o provider **ChatGPT (Codex CLI)**. O app não faz o login nem guarda
token: ele chama o `codex exec` com as ferramentas desligadas, só para
traduzir. Os textos do jogo vão para a OpenAI, então a opção exige "permitir
tradução remota". Cada lote é uma chamada do Codex — projeto grande gasta o
limite da assinatura.

```bash
pnpm install
pnpm dev          # abre o app (tauri dev)
```

Fluxo no app: selecionar arquivo → criar projeto → **Extração estruturada** (ou
scan genérico) → configurar provider e **Traduzir** → revisar no editor →
**Reinserir na cópia de trabalho** → **Exportar patch** (IPS ou BPS). Os artefatos saem
em `SeuJogo.rtsproj/exports/`.

Fixtures sintéticas para testar sem ROM real:

```bash
cargo run -p romtranslate-core --example gen_fixtures
# gera 21 arquivos em fixtures/generated/ — um por plataforma, mais os de
# tabela de ponteiros, tabela de caracteres própria, texto comprimido, lixo
# imprimível antes do texto e texto ASCII com nomes internos
```

Testes e checks:

```bash
cargo test --workspace
cargo fmt --check && cargo clippy --all-targets -- -D warnings
pnpm test && pnpm lint && pnpm web:build
# prova de fogo com Ollama rodando:
cargo test -p romtranslate-core --test translate -- --ignored --nocapture
# a mesma, com o texto numa tabela de caracteres (mostra o que o modelo
# respondeu, o que foi gravado, quantas passaram do espaço e os avisos):
cargo test -p romtranslate-core --test table_translate -- --ignored --nocapture
# a mesma medição com o ChatGPT pelo Codex CLI logado (gasta a cota da conta;
# só as 7 frases sintéticas saem da máquina). TABLE_TEST_NO_LIMIT=1 manda o
# pedido sem o espaço de cada frase, pra comparar:
TABLE_TEST_PROVIDER=codex cargo test -p romtranslate-core --test table_translate -- --ignored --nocapture
```

## Sem a janela (CLI)

Os mesmos passos dos botões do app, pelo terminal: para automatizar, testar e
medir sem abrir a janela. A CLI é uma casca sobre o `crates/core` — nenhuma
lógica própria.

*The same steps as the app's buttons, from the terminal: a thin shell over
`crates/core`, for automation, tests and measurements without the window.*

```bash
cargo run -p romtranslate-cli -- <comando>   # binário: romtranslate
```

| comando | o que faz | what it does |
|---|---|---|
| `inspect <arquivo>` | formato, plataforma e SHA-256 | format, platform and SHA-256 |
| `create <arquivo> <projeto> [--target pt-BR] [--source en-US]` | cria o projeto; o original não é copiado nem alterado | creates the project; the original is never copied or modified |
| `extract <projeto>` | extração estruturada do adapter | the adapter's structured extraction |
| `scan <projeto> [--encoding ascii\|utf8\|utf16_le\|utf16_be\|table] [--tbl arquivo] [--region início..fim] [--min-chars N] [--only-text]` | scanner genérico; `--region` restringe a busca a um trecho do arquivo (bytes, aceita `0x`) | generic scanner; `--region` limits the search to a byte range (`0x` accepted) |
| `export <projeto> <arquivo> [--format json\|csv]` | as frases do projeto, com os ids | the project's entries, with their ids |
| `estimate <projeto> [--batch N]` | tamanho da rodada de tradução, sem chamar o provider | size of the translation round, without calling the provider |
| `translate <projeto> [--batch N] [--parallel N] [--shorten-rounds N]` | traduz as pendentes com o provider das configurações do app; `--parallel` são as chamadas ao mesmo tempo (padrão 4) e `--shorten-rounds` quantas vezes a tradução que não coube volta ao provider (padrão 2; 0 desliga) | translates pending entries with the provider from the app's settings; `--parallel` is the number of simultaneous calls (default 4) and `--shorten-rounds` how many times a translation that does not fit goes back to the provider (default 2; 0 turns it off) |
| `apply <projeto> <arquivo.tsv>` | grava traduções (`id<TAB>tradução` por linha) e valida | writes translations (`id<TAB>translation` per line) and validates |
| `validate <projeto>` | valida, com a contagem por tipo de problema | validates, with a count per kind of issue |
| `reinsert <projeto> [--policy block\|skip\|force]` | grava na cópia de trabalho e confere (padrão `skip`: frase com erro fica no original) | writes to the working copy and verifies (default `skip`: an entry with an error keeps the original) |
| `patch <projeto> [--format ips\|bps]` | exporta patch, manifest e CSV | exports patch, manifest and CSV |

- `--json` em qualquer comando: o resultado numa linha de JSON no stdout.
  Erro vai para o stderr com saída `1` (`2` para argumento inválido).
  *`--json` on any command prints the result as one line of JSON on stdout.
  Errors go to stderr with exit code `1` (`2` for invalid arguments).*
- `translate` lê as configurações do app (mesma pasta; `--config-dir` aponta
  outra), mostra o progresso no stderr e, se o provider parar, diz o motivo e
  sai com `1`. Ctrl-C para depois das chamadas em andamento.
  *`translate` reads the app's settings (same folder; `--config-dir` points
  elsewhere), prints progress on stderr and, when the provider stops, says why
  and exits `1`. Ctrl-C stops after the calls in flight.*

O ciclo inteiro numa fixture sintética / *the whole cycle on a synthetic fixture*:

```bash
cargo run -p romtranslate-core --example gen_fixtures
alias rt='cargo run -q -p romtranslate-cli --'
rt create fixtures/generated/synthetic_pointers.gba /tmp/demo.rtsproj
rt extract /tmp/demo.rtsproj
rt export /tmp/demo.rtsproj /tmp/frases.json   # os ids / the ids
printf 'scan-00000120\tPOCAO\n' > /tmp/traducao.tsv
rt apply /tmp/demo.rtsproj /tmp/traducao.tsv
rt reinsert /tmp/demo.rtsproj
rt patch /tmp/demo.rtsproj
```

## Arquitetura

```
apps/desktop/        # Tauri 2 + React + TS (shell fino; zero lógica de domínio)
apps/cli/            # CLI `romtranslate` (outra casca fina sobre o core)
crates/core/         # tudo testável sem UI
  src/adapter.rs     # trait GameAdapter (probe/extract/apply/verify)
  src/adapters/      # rtsf (referência completa), gba, nds, nes, snes, gamecube,
                     # ps1, ps2, psp; wii e wiiu só detectam
  src/scan.rs        # scanner genérico (Camada A) + a regra de "parece texto"
  src/tbl.rs         # tabelas .tbl: leem o jogo e regravam a tradução
  src/relative.rs    # busca relativa: descobre a tabela de caracteres
  src/xray.rs        # raio-X: o que o app não lê (comprimido, denso, sem ponteiro)
  src/lz77.rs        # LZ77 do BIOS do GBA/DS (leitura; base da Camada D)
  src/pipeline.rs    # tradução: TM → batches → retry → cancel
  src/db.rs          # entries + TM + glossário (SQLite por projeto)
  src/validate.rs    # placeholders/bytes/statuses
  src/reinsert.rs    # working copy + verify
  src/patch.rs       # IPS + BPS create/apply + manifest
  src/settings.rs    # configurações do app + provider montado a partir delas
```

## Contribuindo

O melhor lugar para contribuir é um **adapter de plataforma** — o guia completo
está em [docs/ADAPTERS.md](ADAPTERS.md) e o processo em
[CONTRIBUTING.md](../CONTRIBUTING.md). A spec completa do produto vive em
[docs/SPEC.md](SPEC.md).

Reporte compatibilidade de jogos e peça adapters pelos
[issue templates](../.github/ISSUE_TEMPLATE) — **nunca anexe ROMs, BIOS ou keys**.

## Aviso legal

RomTranslate Studio é uma ferramenta de tradução e modificação para arquivos de
jogos obtidos legalmente pelo próprio usuário. O projeto **não distribui** jogos,
BIOS, keys, firmware ou conteúdo proprietário, e não inclui mecanismos para
baixá-los. Incentivamos a distribuição de **patches** em vez de imagens completas
modificadas e o respeito às leis e licenças aplicáveis.

Licença: [MIT](../LICENSE).
