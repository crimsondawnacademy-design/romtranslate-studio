---
projeto: romtranslate-studio
handoff_gerado: 2026-10-12
gerado_por: claude / opus-5.5
motivo: pausa-longa
branch: main
ultimo_commit_de_codigo: 929503b (2026-09-21)
versao_publicada: v0.1.1
---

# HANDOFF — RomTranslate Studio

Pra quem pega o projeto frio: leia as seções **1 a 3**. O resto é consulta.
O estado vivo fica em `.ctx/STATE.md`; o porquê de cada decisão, em
`.ctx/DECISIONS.md`; a especificação do produto, em `docs/SPEC.md`.

Este documento foi gerado a partir do repositório no GitHub, não de memória de
conversa. O código não muda desde 21/09/2026.

---

## 1. O que é, e onde está

Aplicativo desktop open source (licença MIT) para traduzir jogos retrô com IA.
Ele detecta a plataforma do arquivo, extrai os textos, traduz com IA local
(Ollama) ou com qualquer endpoint compatível com a API da OpenAI, valida as
traduções, reinsere numa cópia de trabalho e exporta um patch (IPS ou BPS).
Dois princípios: **local-first** (o arquivo do jogo nunca sai do computador) e
**patch-first** (distribui-se o patch, nunca a ROM).

É um **projeto pessoal** do Rhuan Strele. Nada dele vai para a infraestrutura
de trabalho do mantenedor (decisão registrada em `.ctx/DECISIONS.md`).

| | |
|---|---|
| Repositório (público) | https://github.com/crimsondawnacademy-design/romtranslate-studio |
| Versão publicada | https://github.com/crimsondawnacademy-design/romtranslate-studio/releases/tag/v0.1.1 |
| Stack | Tauri 2, React 19, Vite 8, TypeScript (pnpm); Rust em workspace |
| Tamanho | 8.650 linhas no core, 3.482 de testes, 610 na ponte Tauri, 1.860 de frontend |
| Testes | 127 em Rust (125 rodam; 2 são manuais) e 9 no frontend |
| CI | verde no commit `929503b` (jobs `rust`, `desktop`, `frontend`) |

A versão publicada é marcada como pré-lançamento (alpha). Por isso o endereço
`/releases/latest` **não** leva a ela: para divulgar, use o link da tag.

**Para começar a trabalhar**, clone do GitHub. Cópias locais antigas podem não
existir mais (limpeza de disco) ou estar com o histórico antigo; veja a
armadilha 7.14.

```bash
git clone https://github.com/crimsondawnacademy-design/romtranslate-studio.git
```

---

## 2. Estado atual (12 de outubro de 2026)

O ciclo completo funciona de ponta a ponta: detectar, extrair, traduzir,
validar e revisar, reinserir em cópia de trabalho e exportar patch.

| Plataforma | Extração | Reinserção | Tradução maior que o original |
|---|---|---|---|
| RTSF (formato sintético próprio, para testes) | completa | completa | sim |
| Game Boy Advance | ASCII | no lugar; checksum do header recalculado | sim: vai para o fim do ROM |
| Nintendo DS | ASCII e UTF-16LE, por arquivo | no lugar; CRC do header recalculado | sim: o arquivo cresce e a FAT é reapontada |
| NES | ASCII (ou tabela `.tbl` no scanner) | no lugar | não |
| Super Nintendo | ASCII | no lugar; checksum interno recalculado | não |
| GameCube | por arquivo do disco | no lugar | não |
| PlayStation 1 (ISO, BIN raw, `.cue`) | por arquivo do disco | no lugar; EDC/ECC regenerado | sim, pouco: sobra do último setor do arquivo |
| PlayStation 2 (inclui DVD acima de 2 GiB) | por arquivo do disco | no lugar | não, de propósito |
| PSP | por arquivo do disco | no lugar | não, de propósito |
| Wii e Wii U | só detecção | não | não |

"No lugar" quer dizer que a tradução ocupa o espaço da frase original. Todas
as plataformas reais são declaradas como **experimentais**.

A tradução maior que o original ("relocação de ponteiros") só vale para frases
apontadas por uma **tabela** de ponteiros que o app reconhece. Sem tabela, a
tradução precisa caber no espaço original, e o app avisa antes de gravar.

Também prontos: memória de tradução por projeto e global, glossário, editor
com revisão, validação (placeholders, limite de bytes, tradução vazia,
tamanho anormal), modo avançado (reinserir mesmo com erros), chave de API no
cofre do sistema operacional, interface em português e inglês, CI de release
que gera binários para macOS (Apple Silicon e Intel), Windows e Linux.

**No código mas ainda não publicado em binário** (commit `929503b`, depois da
v0.1.1): tabelas com ponteiros de 16 bits e a correção descrita em 3.1.

---

## 3. Pendências — por prioridade

### 3.1 Decisão pendente: lançar a v0.1.2

A v0.1.1 publicada tem um bug que o commit `929503b` corrige. Ele aparece
num caso específico de DS ou PS1: o arquivo começa com uma frase, tem uma
sequência de zeros em algum lugar, e essa primeira frase é traduzida para algo
maior que o original. O app tratava os zeros como tabela de ponteiros e os
reescrevia. Se forem só preenchimento, nada acontece; se forem dado do jogo, o
jogo corrompe. É raro, mas é corrupção silenciosa.

Falta só decidir e executar o passo a passo da seção 6.3. Ao publicar, vale
pôr na v0.1.1 o mesmo tipo de aviso que a v0.1.0 já tem.

### 3.2 Prioridade 1: nada foi testado em jogo real

Todo teste usa arquivos sintéticos gerados por código (`crates/core/src/synth.rs`).
Eles provam a mecânica, não o comportamento em jogo de verdade. Perguntas em
aberto, na ordem em que eu mediria:

1. Quantas frases de um jogo real ficam em tabela que o app reconhece? Ponteiro
   solto no meio do código e, no PS1, endereço montado em duas instruções não
   são detectados.
2. No DS, a regra que escolhe entre leitura ASCII e UTF-16 acerta em texto
   japonês real? Um texto só de kanji pode ser lido do jeito errado (há um
   comentário `ponytail:` em `adapters/nds.rs` sobre isso).
3. No DS, o jogo lê o arquivo que cresceu? Jogo que reserva memória de tamanho
   fixo para o arquivo não veria o texto novo.
4. No PS1, o jogo lê a sobra do setor? Jogo que copia um tamanho fixo não leria.
5. O caminho de DVD de PS2 acima de 2 GiB funciona com um dump real? Hoje ele
   é coberto por um teste que força esse caminho num arquivo pequeno.

Teste com um dump seu, **fora do repositório**. Nenhum byte de jogo entra no
repositório, nem em teste.

### 3.3 Prioridade 2: tabela de ponteiros indicada pelo usuário

Hoje o app só reconhece tabelas sozinho. Falta o usuário poder dizer "a tabela
fica aqui, com este formato", como na ferramenta Atlas. Isso destrava NES e
SNES (o ponteiro de 16 bits por banco é ambíguo demais para detecção
automática) e as tabelas que a detecção recusa de propósito: fora de ordem, ou
com posição relativa a uma seção do arquivo em vez do arquivo inteiro.

### 3.4 Prioridade 2: limites de formato que travam uso real

- **Sem descompressão.** Texto comprimido, comum em RPG japonês, não aparece
  na extração.
- **Sem Shift-JIS.** O tipo existe (`TextEncoding::ShiftJis`), mas o scanner
  não decodifica e a reinserção ignora essas entradas.
- **Sem edição de fonte.** Se o jogo não tem os glifos de á, ç, ã, a tradução
  sai sem acento.
- **Wii e Wii U só detectam.** As partições são cifradas e o projeto não
  inclui chaves, por decisão.

### 3.5 Prioridade 2: divulgação

O projeto nunca foi divulgado. Em 12/10/2026: 2 estrelas, nenhuma issue,
nenhum pull request. Os lugares naturais são r/romhacking e ROMhacking.net.
Divulgue depois da v0.1.2, com o link da tag.

### 3.6 Miudezas

- O gerador de arquivos de teste (`cargo run -p romtranslate-core --example gen_fixtures`)
  não gera os três arquivos com tabela de ponteiros
  (`make_gba_rom_with_pointers`, `make_nds_rom_with_message_table`,
  `make_ps1_bin_with_message_table`). Sem eles não dá para experimentar a
  relocação na interface sem um jogo real. São três entradas a mais na lista
  de `crates/core/examples/gen_fixtures.rs`.
- O `README.md` está defasado em quatro pontos: linha 11 e linha 139 falam só
  em patch IPS (existe BPS); a linha 146 lista 5 extensões de arquivo de teste
  (o gerador grava 14 arquivos); a linha 165 lista 4 adapters (são 11).
- `apps/desktop/README.md` ainda é o texto do template do Tauri.
- Os lotes de tradução rodam um por vez; rodar em paralelo foi adiado.
- Arquivo `.cue` fora do padrão Redump (dados que não começam no início do
  arquivo) é recusado com mensagem clara.
- A versão do Rust não é fixada: o CI usa a estável do dia e o `clippy` roda
  com `-D warnings`. Depois de uma pausa longa, um lint novo pode quebrar o CI
  sem ninguém ter mexido no código.

---

## 4. Arquitetura

Regra central: **`crates/core` tem toda a lógica e nunca depende do Tauri.**
A interface é uma casca fina sobre comandos. Tudo no core é testável com
`cargo test`.

**Adapter de plataforma.** Cada plataforma implementa o trait `GameAdapter`
(`adapter.rs`): `probe` (devolve confiança de 0 a 1 e evidências legíveis),
`list_resources`, `extract_structured`, `apply_text` e `verify`. O que o
adapter sabe fazer é declarado em `capabilities()`. O registro fica em
`adapters/mod.rs`, do formato mais específico para o mais genérico. O guia para
escrever um adapter é `docs/ADAPTERS.md`.

**Caminho de um arquivo pelo app:**

1. `detect::inspect` calcula o SHA-256 e roda o `probe` de todos os adapters
   sobre os primeiros 128 KiB. Um `.cue` é resolvido antes para o arquivo do
   track de dados (`cue.rs`).
2. `project::create_project` cria a pasta `.rtsproj` (manifesto, banco SQLite e
   subpastas). A ROM **nunca** é copiada: guardam-se o caminho e o hash.
3. Extração: o scanner genérico (`scan.rs`) serve para descoberta;
   `extract_structured` do adapter produz entradas (`TextEntry`) com o limite
   real de bytes e, quando acha tabela, os ponteiros.
4. `pipeline::run_translation` consulta a memória de tradução do projeto,
   depois a global, e manda o resto ao provedor em lotes, com nova tentativa e
   cancelamento. Os provedores ficam em `providers/`.
5. `validate.rs` confere cada tradução. Erro bloqueia a reinserção.
6. `reinsert::reinsert_project` confere o hash da origem, valida de novo,
   chama `apply_text`, grava em `working/` de forma atômica, relê do disco e
   roda `verify`. É tudo ou nada.
7. `patch::export_patch` gera IPS ou BPS, confere que o patch reproduz a cópia
   de trabalho e grava patch, manifesto e CSV em `exports/`.

**Reinserção no lugar** (`adapters/inplace.rs`). `plan_in_place` confere que
os bytes atuais ainda são os da extração, monta as escritas e recusa duas
traduções nos mesmos bytes. Tradução que não cabe vira pedido de relocação
quando o adapter permite e a entrada tem ponteiros; senão é erro pedindo texto
menor.

**Ponteiros e relocação** (`adapters/pointers.rs`). Um `PointerFormat` descreve
a tabela: largura (4 ou 2 bytes), base, quantos ponteiros seguidos são exigidos
e se a posição é relativa. `find_pointer_tables` acha tabelas; `relocate` grava
o texto novo, reaponta, e confere o resultado. Onde o texto novo vai é decisão
de cada plataforma: fim do ROM (GBA, em `gba.rs`), fim do arquivo que cresce
(DS, `relocate_files` em `nds.rs`), sobra do último setor (PS1,
`relocate_into_slack` em `iso9660.rs`). Os ponteiros de uma entrada ficam no
`metadata`: `pointers` (32 bits) e `pointers16` (16 bits).

**Discos.** `adapters/iso9660.rs` é compartilhado por PS1, PS2 e PSP e lê
imagem de 2048 bytes por setor e BIN raw de 2352. `adapters/cdrom.rs` regenera
EDC e ECC dos setores alterados. Arquivos grandes são lidos por mapeamento em
memória (`fileio.rs`), e acima de 2 GiB a reinserção copia o arquivo e escreve
só os trechos alterados.

**Dados.** Cada projeto tem um `translations.sqlite` (entradas, memória de
tradução, glossário, histórico de execuções). A memória global fica num SQLite
na pasta de configuração do app. Configurações em `settings.toml`; a chave de
API vai para o cofre do sistema, com arquivo de permissão restrita só como
plano B.

---

## 5. Estrutura de arquivos

```
crates/core/src/
  adapter.rs        trait GameAdapter, limites de tamanho, relatórios
  adapters/
    mod.rs          registro dos adapters
    inplace.rs      reinserção no lugar (plano de escritas)
    pointers.rs     tabelas de ponteiros e relocação
    iso9660.rs      sistema de arquivos de disco (PS1, PS2, PSP)
    cdrom.rs        EDC/ECC de setor de CD
    rtsf.rs         formato sintético de referência
    gba.rs nds.rs nes.rs snes.rs gamecube.rs ps1.rs ps2.rs psp.rs
    wii.rs wiiu.rs  só detecção
  detect.rs         inspeção (hash + probes)
  cue.rs            leitura de cue sheet
  scan.rs tbl.rs    scanner genérico e tabelas .tbl
  project.rs        projeto .rtsproj
  db.rs             SQLite do projeto e memória global
  provider.rs       trait do provedor, prompt, leitura da resposta
  providers/        ollama.rs, openai_compat.rs
  pipeline.rs       execução da tradução
  validate.rs       validação e status das entradas
  reinsert.rs       orquestração da reinserção
  patch.rs          IPS e BPS
  fileio.rs         leitura por mapeamento em memória
  export.rs hash.rs types.rs error.rs
  synth.rs          arquivos sintéticos de teste (nenhum byte de jogo real)
crates/core/tests/  16 arquivos de teste de integração
crates/core/examples/gen_fixtures.rs   grava os arquivos sintéticos em disco
apps/desktop/src-tauri/src/
  lib.rs            os 21 comandos expostos à interface
  settings.rs       configurações e chave de API
apps/desktop/src/
  App.tsx           fluxo inicial: escolher arquivo, inspecionar, criar projeto
  ProjectView.tsx   a tela do projeto inteira
  i18n.ts           textos em pt-BR e en-US (um teste exige as mesmas chaves)
  types.ts util.ts  espelho dos tipos do core e utilitários
.github/workflows/  ci.yml (3 jobs) e release.yml (4 plataformas)
docs/               SPEC.md (produto), ADAPTERS.md (guia), assets/
.ctx/               STATE.md, DECISIONS.md, HANDOFF.md, TOOL-LOG.md
```

---

## 6. Como rodar, validar e publicar

### 6.1 Rodar

Pré-requisitos: Rust estável, Node 20 ou mais novo (o CI usa 24), pnpm (o CI
usa 12) e as dependências de sistema do Tauri 2. Para traduzir localmente,
Ollama com um modelo baixado (por exemplo `ollama pull llama3.2:3b`); o campo
de modelo começa vazio e precisa ser preenchido na tela.

```bash
pnpm install
pnpm dev                                              # abre o app
cargo run -p romtranslate-core --example gen_fixtures # arquivos de teste em fixtures/generated/
```

A primeira compilação cria `target/`, que já chegou a 13 GB.

### 6.2 Validar (tudo precisa passar antes de qualquer commit)

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
pnpm lint && pnpm test && pnpm web:build
```

Dois testes são manuais e ficam fora do CI:

```bash
cargo test -p romtranslate-core --test translate -- --ignored --nocapture  # exige Ollama rodando
cargo test -p romtranslate-desktop -- --ignored                            # grava no cofre do sistema
```

### 6.3 Publicar uma versão

1. Suba o número em três arquivos: `Cargo.toml`, `apps/desktop/package.json` e
   `apps/desktop/src-tauri/tauri.conf.json`. Rode `cargo build` para atualizar
   o `Cargo.lock`.
2. Valide (6.2), faça o commit e envie.
3. Crie e envie a tag: `git tag vX.Y.Z && git push origin vX.Y.Z`. O primeiro
   passo do workflow barra a tag se ela não bater com o `tauri.conf.json`.
4. O workflow compila as quatro plataformas e abre um release em **rascunho**
   com 9 arquivos. É rascunho de propósito: se uma plataforma falhar, nada
   incompleto vai ao ar.
5. Acrescente as novidades no topo das notas (`gh release edit vX.Y.Z --notes-file ...`).
   O corpo padrão, com as instruções de instalação, vem do próprio workflow.
6. Publique: `gh release edit vX.Y.Z --draft=false`.
7. Confira sem login que a página abre e que um instalador baixa.

Para testar o workflow sem criar tag nem release: `gh workflow run release.yml`.
Ele só compila e deixa os binários na página da execução.

Os binários não são assinados (certificado é pago). No macOS o sistema diz que
o app "está danificado"; a saída é `xattr -cr "/Applications/RomTranslate Studio.app"`.
Isso está no README e no corpo de todo release.

---

## 7. Armadilhas que custaram tempo

**7.1 Nunca anexe texto a algo que o jogo copia para a memória.** O binário
principal do DS (ARM9) e o executável do PS1 são copiados com tamanho fixo, e
logo depois deles ficam a área que o jogo zera ao ligar e a memória de uso
geral. Texto anexado ali some. O GBA só é seguro porque o cartucho é lido
direto pelo processador. Eu cheguei a registrar o contrário; a correção está
em `.ctx/DECISIONS.md`.

**7.2 Nunca troque ponteiro por busca global.** No GBA, uma instrução comum
(`LSR`, em Thumb) pode ter o mesmo valor de um ponteiro de ROM. Trocá-la
corrompe o jogo sem aviso. Só vale ponteiro dentro de tabela.

**7.3 Posição relativa ao arquivo é número pequeno, e número pequeno aparece à
toa.** Por isso a tabela relativa exige ordem crescente, ignora ponteiro para a
posição 0, e pede 3 ponteiros seguidos em 32 bits e 4 em 16 bits. Sem essas
regras, uma faixa de zeros virava "tabela" apontando para a frase que abre o
arquivo.

**7.4 Uma leitura por trecho de bytes.** O DS é varrido em ASCII e em UTF-16.
Texto ASCII lido como UTF-16 vira caracteres chineses falsos na mesma posição,
com o mesmo identificador (que é só a posição). No banco as duas leituras se
fundiam e a tradução era gravada no formato errado. A correção é
`resolve_encoding_overlaps` em `nds.rs`. Qualquer adapter que varra dois
formatos precisa do mesmo cuidado.

**7.5 No ECC de CD, o segundo passo lê o resultado do primeiro.** No código C
de referência os ponteiros se sobrepõem. Em Rust são duas passadas com fatias
distintas (`cdrom.rs`). Os valores de referência foram gerados compilando o
programa original e estão fixados em teste.

**7.6 IPS tem dois limites.** Um registro não pode começar na posição
`0x454F46`, que é lida como fim de arquivo, e o formato não alcança além de
16 MiB. O export escolhe BPS sozinho quando o IPS não serve.

**7.7 Valide em primeiro plano antes do commit.** Um commit já foi enviado sem
compilar porque a validação rodava em segundo plano e o CI só cobria o core.
Hoje o CI compila o app inteiro.

**7.8 O servidor de desenvolvimento usa a porta 1420**, fixa, porque o Tauri
exige.

**7.9 Para ver a interface sem o backend**, simule `window.__TAURI_INTERNALS__`
(funções `invoke` e `transformCallback`) e
`window.__TAURI_EVENT_PLUGIN_INTERNALS__` no navegador. A captura de tela do
README foi feita assim, com dados fictícios.

**7.10 Contador de clones do GitHub não mede pessoas.** Cada job do CI faz um
clone, e repositório público novo é varrido por robôs. O contador de downloads
também conta as checagens feitas por linha de comando. Meça interesse por
visitas e por sites de origem.

**7.11 `original_bytes` atravessa a ponte como texto hexadecimal**, não como
lista de bytes.

**7.12 Confira o formato na fonte antes de programar.** Todo número mágico do
projeto veio de fonte primária (seção 9). Formato sem fonte confiável ficou de
fora em vez de ser chutado.

**7.13 Projeto de DS criado antes da v0.1.1 precisa refazer a extração
estruturada.** As traduções são preservadas.

**7.14 O histórico do repositório foi reescrito em 15/09/2026**, antes de abrir
ao público. Clone antigo deve ser apagado e clonado de novo, nunca atualizado
por cima.

---

## 8. Regras que valem para qualquer sessão

- `crates/core` não depende do Tauri.
- Parser de binário confere limites em todo acesso e nunca derruba o programa
  com entrada malformada. Teste com entrada truncada, vazia e aleatória.
- O arquivo original do usuário nunca é copiado para o projeto nem modificado.
- Nenhuma ROM comercial, BIOS, chave ou byte de jogo real no repositório.
- Nenhum segredo em log, configuração versionada ou repositório.
- Sem download de jogos, sem link para ROM, sem promessa de suporte universal.
  Capacidade declarada tem que ter teste de ida e volta.
- Todo texto da interface existe em português e em inglês.
- Projeto pessoal: nada vai para a infraestrutura de trabalho do mantenedor.
- Ao terminar: atualize `.ctx/STATE.md` e `.ctx/TOOL-LOG.md`, registre decisão
  nova em `.ctx/DECISIONS.md` (só acrescentando), faça commit e envie.

---

## 9. Referências

- `docs/SPEC.md`: especificação do produto e roteiro original de 8 etapas, todo
  entregue.
- `docs/ADAPTERS.md`: como escrever um adapter.
- `.ctx/DECISIONS.md`: 30 decisões, com contexto e consequência.
- Fontes dos formatos: GBATEK (GBA e DS), código do Dolphin (GameCube e Wii),
  Cemu e decaf-emu (Wii U), `iso_fs.h` do kernel Linux (ISO 9660), `ecm.c` de
  Neill Corlett (EDC/ECC), psdevwiki (PSP), repositório da `tauri-action`
  (workflow de release).

## Commits mais recentes (o mais novo primeiro)

```
929503b  feat(ponteiros): offsets u16 nas tabelas + corrige padding virando tabela
6c2998a  ctx: aviso na v0.1.0 apontando pra v0.1.1
75c47ee  ctx: v0.1.1 publicada e conferida sem login
3274590  chore: v0.1.1
52947e6  feat(nds,ps1): relocacao dentro dos arquivos + corrige leituras fundidas no NDS
ab24e5d  feat(gba): relocacao de ponteiros — traducao maior vai pro fim do ROM
9a5965a  ctx: v0.1.0 publicado e conferido sem login
```

---
*Gerado por claude (opus-5.5) em 12/10/2026. Próxima ferramenta: leia
`.ctx/STATE.md` antes de editar e `.ctx/TOOL-LOG.md` para o histórico.*
