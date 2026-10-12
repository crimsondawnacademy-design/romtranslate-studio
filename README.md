# RomTranslate Studio

> Local-first, AI-assisted game translation toolkit.

Ferramenta desktop open source para extrair, traduzir, revisar e reaplicar textos em
jogos compatíveis, com IA local (Ollama) ou APIs configuráveis, gerando **patches** (IPS/BPS)
em vez de cópias modificadas.

**Status: alpha.** O pipeline completo funciona de ponta a ponta:
detecção → extração → tradução com IA → validação/revisão → reinserção em
cópia de trabalho → **patch IPS ou BPS** com manifest.

![Tela de projeto do RomTranslate Studio: extração de strings de um ISO de PS2 com traduções, statuses e validação](docs/assets/screenshot.png)

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
- **Extração**: scanner genérico (ASCII, UTF-8, UTF-16 LE/BE, tabelas `.tbl`)
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
  OpenAI-compatible, ou a sua assinatura do ChatGPT pelo Codex CLI (sem chave
  de API); batching, retry, cancelamento, progresso; **translation
  memory** e **glossário** por projeto (SQLite); tradução remota exige opt-in
  explícito de privacidade;
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
- extrair de novo um projeto antigo não tira dele o lixo que já estava lá.
  Para uma lista limpa, crie um projeto novo: a memória de tradução devolve o
  que já foi traduzido;
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
com tabela. Com um modelo de verdade só há medição num modelo local pequeno
(`llama3.2:3b`, as 7 frases da fixture): nenhuma pontuação fora da tabela, e
4 respostas ainda vieram com acento e foram gravadas sem ele. A troca só
cobre letra latina acentuada num caractere só (`á`, `ç`, `É`); tradução
escrita à mão não é mexida.

**Tamanho do texto de tabela**: ele é sempre gravado no lugar, então o
espaço da original é o teto. A tradução por IA recebe esse teto em cada
frase, em caracteres (`maxChars`), com a instrução de que é limite duro. O
número é contado pela entrada mais cara da tabela: numa tabela de 1 byte por
caractere é o próprio espaço; se algum caractere custa 2 bytes, é a metade —
nunca mais do que cabe, às vezes menos. Quem decide se cabe é a validação,
que mede em bytes da tabela: tradução maior que o espaço é **erro** na frase
(antes era aviso, e a recusa só aparecia na reinserção), com os dois
tamanhos. O app **não corta** texto: a tradução fica inteira no projeto até
você encurtar. O que foi medido: o limite no pedido não fez o `llama3.2:3b`
caber. Sem ele, as 7 traduções passavam do espaço; com ele, 6 de 7
continuaram passando nas 4 rodadas (somando as 7 frases, 17 a 26
caracteres a mais, contra 30), e a única que coube perdeu parte do sentido. Modelo maior não foi medido — conte com o
erro de validação e com encurtar à mão. Limite conhecido: string de largura
fixa numa tabela sem espaço precisa do tamanho exato, e o pedido só diz o
máximo. E a sobra de
uma tradução mais curta vira `0x00` quando o byte depois da string original
é `0x00` (senão é preenchida com o espaço da tabela) — jogo com outro
terminador pede conferência no emulador.

Se o texto está comprimido, o **Raio-X** mostra quanto — hoje o app só
reconhece o LZ77 tipo 0x10 e só para leitura: texto comprimido ainda não é
extraído nem reinserido.

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
# gera 20 arquivos em fixtures/generated/ — um por plataforma, mais os de
# tabela de ponteiros, tabela de caracteres própria, texto comprimido e lixo
# imprimível antes do texto
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
```

## Arquitetura

```
apps/desktop/        # Tauri 2 + React + TS (shell fino; zero lógica de domínio)
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
```

## Contribuindo

O melhor lugar para contribuir é um **adapter de plataforma** — o guia completo
está em [docs/ADAPTERS.md](docs/ADAPTERS.md) e o processo em
[CONTRIBUTING.md](CONTRIBUTING.md). A spec completa do produto vive em
[docs/SPEC.md](docs/SPEC.md).

Reporte compatibilidade de jogos e peça adapters pelos
[issue templates](.github/ISSUE_TEMPLATE) — **nunca anexe ROMs, BIOS ou keys**.

## Aviso legal

RomTranslate Studio é uma ferramenta de tradução e modificação para arquivos de
jogos obtidos legalmente pelo próprio usuário. O projeto **não distribui** jogos,
BIOS, keys, firmware ou conteúdo proprietário, e não inclui mecanismos para
baixá-los. Incentivamos a distribuição de **patches** em vez de imagens completas
modificadas e o respeito às leis e licenças aplicáveis.

Licença: [MIT](LICENSE).
