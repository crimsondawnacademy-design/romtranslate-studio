---
projeto: romtranslate-studio
handoff_gerado: 2026-10-13
gerado_por: claude / opus-5.5
motivo: fim-de-sessao-longa (janela de contexto fechando)
branch: main
ultimo_commit: c2cec40 (2026-10-13)
versao_publicada: v0.1.5
numero: 4
adendo: 2026-10-13 à noite (v0.1.6 e japonês) — ler antes do resto
---

> ## Adendo de 13/10/2026 à noite — o que mudou depois deste handoff
>
> O corpo abaixo descreve a v0.1.5. Depois dele, no mesmo dia:
>
> - **v0.1.6 publicada** (tag em `4f09522`): disco de PS2, PS1 e PSP
>   reconhecido mesmo com o índice de arquivos depois dos 128 KiB do probe
>   (primeiro bug relatado por alguém de fora, com um DVD de PS2 de 2,7 GiB);
>   extração de disco grande sem estourar a memória e sem encher o projeto de
>   lixo; Shift-JIS; texto comprimido LZ77 no GBA.
> - **No `main`, depois da tag (sem binário):** japonês em códigos de 16 bits
>   na ordem JIS (`crates/core/src/kuten.rs`), com a tabela gerada e gravada
>   no projeto; letra em largura inteira na gravação por tabela;
>   `ProjectDb::get_entry` por linha.
> - **A prioridade do mantenedor passou a ser jogo em japonês.** O primeiro
>   jogo japonês real medido não usa Shift-JIS: o app lê 11.676 mensagens dele
>   pelo formato de 16 bits. Gravar de volta é o gargalo (2 bytes por letra,
>   frase que não muda de lugar, kanji de lista própria saindo trocados) e
>   ninguém viu uma tradução dessas rodando.
> - **Módulos novos:** `adapters/packed.rs` (texto em bloco LZ77), `sjis.rs`
>   (Shift-JIS e a regra de texto japonês, `Script`), `kuten.rs`.
>   `ScanConfig` ganhou `japanese` e `skip_noise`; `Extraction` ganhou
>   `tables`; `GameInput` ganhou `whole()`.
> - **Armadilhas novas:** disco sintético montado por nós repete as nossas
>   suposições de layout (conferir com imagem de outra ferramenta:
>   `hdiutil makehybrid -iso -udf`); "jogo japonês" não é um formato (medir
>   num jogo real antes de escrever leitor); publicar o release antes de todos
>   os jobs terminarem faz o `tauri-action` abrir um SEGUNDO rascunho com os
>   arquivos que faltavam (mover os arquivos pela API e apagar o rascunho).
>
> Detalhes, medições e o que não foi conferido: `.ctx/STATE.md`, "Feito nesta
> sessão" e próximo passo 0d; decisões de 13/10 no fim do `.ctx/DECISIONS.md`.

# HANDOFF 4 — RomTranslate Studio

Pra quem pega o projeto frio: leia as seções **1 a 3**. O resto é consulta.
O estado vivo fica em `.ctx/STATE.md`; o porquê de cada decisão, em
`.ctx/DECISIONS.md`; a especificação do produto, em `docs/SPEC.md`.

Este é o quarto handoff. O terceiro (12/10 à noite) descrevia a v0.1.3. Em
13/10 o projeto ganhou o primeiro jogo inteiro traduzido, duas versões, uma
linha de comando, identidade visual nova, página nova no GitHub e um site.

---

## 1. O que é, e onde está

Aplicativo desktop open source (licença MIT) para traduzir jogos retrô com IA.
Ele detecta a plataforma do arquivo, extrai os textos, traduz com IA, valida,
reinsere numa cópia de trabalho e exporta um patch (IPS ou BPS). Dois
princípios: **local-first** (o arquivo do jogo nunca sai do computador; só os
textos saem, e só com autorização) e **patch-first** (distribui-se o patch,
nunca a ROM).

É um **projeto pessoal** do Rhuan Strele. Nada dele vai para a infraestrutura
de trabalho do mantenedor.

| | |
|---|---|
| Repositório (público) | https://github.com/crimsondawnacademy-design/romtranslate-studio |
| Site | https://romtranslate-studio.pages.dev (inglês em `/en/`) |
| Versão publicada | https://github.com/crimsondawnacademy-design/romtranslate-studio/releases/tag/v0.1.5 (13/10/2026, tag no commit `005dc16`, pré-lançamento) |
| Stack | Tauri 2, React 19, Vite, TypeScript (pnpm); Rust em workspace: `crates/core`, `apps/desktop/src-tauri`, `apps/cli` |
| Testes | `cargo test --workspace`: 34 conjuntos, todos verdes; 9 testes no frontend |
| CI | jobs `rust`, `desktop` e `frontend`; release por tag nas 4 plataformas |

Toda versão sai como pré-lançamento (alpha): `/releases/latest` **não** leva a
ela. Para divulgar, use o link da tag ou o site.

---

## 2. Estado atual (13 de outubro de 2026, tarde)

| Plataforma | Extração | Reinserção | Tradução maior que o original |
|---|---|---|---|
| RTSF (formato sintético próprio, para testes) | completa | completa | sim |
| Game Boy Advance | ASCII | no lugar; checksum do header recalculado | sim: vai para o fim do ROM |
| Nintendo DS | ASCII e UTF-16LE, por arquivo | no lugar; CRC do header recalculado | sim: o arquivo cresce e a FAT é reapontada |
| NES | ASCII (ou tabela `.tbl` no scanner) | no lugar; texto lido por `.tbl` volta pela tabela | não |
| Super Nintendo | ASCII | no lugar; checksum interno recalculado | não |
| GameCube | por arquivo do disco | no lugar | não |
| PlayStation 1 (ISO, BIN raw, `.cue`) | por arquivo do disco | no lugar; EDC/ECC regenerado | sim, pouco: sobra do último setor do arquivo |
| PlayStation 2 (inclui DVD acima de 2 GiB) | por arquivo do disco | no lugar | não, de propósito |
| PSP | por arquivo do disco | no lugar | não, de propósito |
| Wii e Wii U | só detecção | não | não |

"No lugar" quer dizer que a tradução ocupa o espaço da frase original. Todas
as plataformas reais são declaradas como **experimentais**. Tradução maior
que o original só vale para frase apontada por uma **tabela** de ponteiros
que o app reconhece.

### 2.1 O que entrou em 13/10

- **Primeiro jogo inteiro.** Um jogo comercial de GBA de 16 MiB (do mantenedor,
  fora do repo) saiu traduzido e ligou num emulador: abertura e primeiro
  diálogo vistos em português, com acento. Ninguém jogou além disso.
- **Tradução em paralelo com repergunta** (`pipeline.rs`): 4 lotes por vez
  (`TranslateOptions.parallel`); o que volta maior que o espaço da frase é
  repedido ao provider com o teto em caracteres (`shorten_rounds`, 2 rodadas);
  25 frases por chamada por padrão.
- **Cancelar na hora**: as chamadas em andamento são largadas em menos de um
  segundo; a tela mostra "Cancelando…".
- **Frase repetida vai uma vez só** ao provider; as cópias recebem a mesma
  tradução.
- **Fila sem lixo**: `scan::stray_ids` acha as entries fora das regiões de
  texto do arquivo e a extração estruturada as tira da tradução
  (`ProjectDb::mark_stray`: viram `Internal` e perdem a tradução automática).
- **CLI** `romtranslate` (`apps/cli`): `inspect`, `create`, `extract`, `scan`
  (com `--tbl`, `--region`, `--min-chars`), `export`, `estimate`, `translate`
  (com `--parallel`, `--shorten-rounds`), `apply`, `validate`, `reinsert`,
  `patch`. Roda do código; não vem no instalador.
- **Identidade visual nova**: cores em tokens no `:root` de `App.css`, tema
  claro como principal e escuro pelo sistema, `logo.svg` como fonte única do
  logo e dos ícones. Simulador do backend em `apps/desktop/.preview/`.
- **Página do GitHub**: `README.md` em inglês e `README.pt-BR.md`; o texto
  técnico foi para `docs/DETALHES.md`; apoio por Pix; `.github/FUNDING.yml`.
- **Site** em `site/`, publicado no Cloudflare Pages (conta Crimson Dawn): a
  demonstração é o app de verdade rodando na página com um jogo inventado.
- **Versões** v0.1.4 (madrugada) e v0.1.5 (tarde), as duas conferidas sem login.

### 2.2 Números medidos (mesmo jogo real, só contagens)

| Medida | Resultado |
|---|---|
| Entries da extração estruturada | 17.936; 2.326 nomes internos; 15.610 pendentes |
| Texto de verdade (banco de mensagens contíguo de 308 KiB) | 11.195 entries, 8.717 textos distintos |
| Fora do banco (gráfico, som, tabela que parece texto) | 4.415; a regra nova tira 4.363 e não marca nenhuma fala |
| Trechos traduzidos e reinseridos (scan com `.tbl` + região) | 10.705, 1 pulado; pelo caminho ASCII da tela, 10.370 |
| Estrutura depois da reinserção | mesmo tamanho; 0 bytes alterados fora de texto; códigos de controle das 3.699 mensagens idênticos |
| Codex CLI, um lote por vez | lote de 25: 250 linhas em 254 s, 93% válidas; lote de 50: 200 em 161 s, 92,5% |
| Codex CLI, 4 em paralelo + repergunta | 200 linhas em 86 s, 199 de 200 válidas |
| Ritmo visto no app do mantenedor (4 x 50) | ~200 linhas por leva, cerca de 5 frases por segundo |

**O volume do jogo inteiro NÃO foi traduzido pelo provider do app:** foram 18
agentes em paralelo fora dele, cada um num laço traduz, confere o tamanho,
corrige. O app fez extração, validação, reinserção e patch.

### 2.3 O caminho que a pessoa faz no app hoje

1. Arrasta o jogo (ou abre o projeto que já existe).
2. **Extração estruturada**: guarda o espaço de cada frase, descarta o que não
   parece texto, marca nomes internos e tira da fila o que está fora das
   regiões de texto. Texto sai como ASCII: sem acento.
3. Escolhe o provider e **Traduzir pendentes**. Dá para cancelar a qualquer
   momento; o que foi traduzido fica.
4. **Reinserir na cópia de trabalho**: frase com erro fica no original.
5. **Exportar patch**.

Acento e as palavras curtas ("No", "The", "to") só entram pelo caminho de
tabela de caracteres com região, que hoje existe **só na CLI**
(`scan --encoding table --tbl <arquivo> --region inicio..fim --min-chars 1`).

---

## 3. Pendências — por prioridade

### 3.1 O que falta para um leigo traduzir um jogo inteiro só pelo app

1. **Traduzir um jogo inteiro com o provider do app.** Nunca foi feito do
   começo ao fim. Com a assinatura do ChatGPT são uns 30 minutos para ~8,9 mil
   textos, se a cota do plano aguentar.
2. **Região e tabela de caracteres na tela.** Sem isso a tradução pela tela
   sai sem acento e com palavras curtas em inglês. O core já faz; falta a
   interface (campo de região, escolher `.tbl`, mínimo de caracteres).
3. **Códigos de controle do jogo declarados junto da tabela.** Hoje um
   parâmetro de código de controle que cai na tabela entra colado no começo do
   texto (101 trechos no jogo medido), e `%` + letra é lido como marcador
   (`PlaceholderMismatch` falso).
4. **Tabela de ponteiros de offset relativo declarada pelo usuário.** É o que
   tira o campo fixo: "Não" não cabe em "No" (39 menus), "mapa" em "map".
5. **Sobra de espaço no meio da linha** (254 casos no jogo medido) e **largura
   em pixels** (49 linhas passaram da caixa e foram encurtadas à mão).
6. **Alguém jogar o jogo traduzido até o fim.**

### 3.2 O que foi construído e ainda não foi visto funcionando de verdade

- **As correções da v0.1.5 no app compilado**: cancelar, "Configuração salva",
  a contagem das strings fora das regiões de texto. Vistas por teste
  automático e por medição numa cópia do projeto real; não clicadas.
- **A regra das regiões de texto fora daquele jogo**: calibrada num GBA só.
  Só age com banco nítido (100+ frases, 70% delas nas regiões). Desfazer é
  entry por entry; a marca é refeita a cada extração.
- **Paralelismo com outros providers**: só o Codex CLI foi medido, e só com 4.
  Ollama local não ganha nada; API com limite de taxa pode recusar.
- **`estimate_translation` não conta a repergunta** (deixou de ser teto).
- **Windows e Linux**: os instaladores compilam no CI; ninguém abriu.
- **Claude por chave de API**: nenhuma chamada real (não havia chave).
- **Site**: visto em Chrome sem janela e no navegador interno; não em Safari,
  Firefox, celular de verdade nem monitor 4K real.
- **CLI**: testes verdes; `--parallel` e `--shorten-rounds` vistos no código.

### 3.3 Decisões que são do mantenedor

- **Claude Code pela assinatura** e **login do OpenRouter**: seguem como no
  handoff 3 (ver `.ctx/DECISIONS.md`, 12/10).
- **Assinatura dos binários** (certificados pagos) e **domínio próprio** do
  site: não decididos.
- **Divulgação** (r/romhacking, ROMhacking.net): o site e a página existem; a
  decisão de quando anunciar é dele.

### 3.4 Lista de trabalho

`.ctx/STATE.md`, "Próximos passos": **0c** (do primeiro uso pela tela: campo
de chamadas ao mesmo tempo e medir 8 no Codex; desfazer em lote das strings
soltas; calibrar a regra num segundo jogo) e **0a** (região na tela, lista de
caracteres vinda do jogo, ponteiro relativo declarado, códigos de controle
declarados, sobra de espaço).

---

## 4. Arquitetura

Regra central: **`crates/core` tem toda a lógica e nunca depende do Tauri.**
A interface é uma casca fina sobre comandos. Tudo no core é testável com
`cargo test`.

**Adapter de plataforma.** Cada plataforma implementa o trait `GameAdapter`
(`adapter.rs`): `probe`, `list_resources`, `extract` (com `extract_structured`
por cima), `apply_text` e `verify`. O registro fica em `adapters/mod.rs`. O
guia para escrever um adapter é `docs/ADAPTERS.md`.

**Caminho de um arquivo pelo app:**

1. `detect::inspect` calcula o SHA-256 e roda o `probe` de todos os adapters.
2. `project::create_project` cria a pasta `.rtsproj`. A ROM nunca é copiada:
   guardam-se o caminho e o hash.
3. `adapters::extract_project` roda o `extract` do adapter, grava as entradas
   e remove o lixo antigo. `scan::looks_like_text` decide o que é texto;
   `scan::looks_like_identifier` marca nome interno (`TranslationStatus::Internal`).
   Entrada com ponteiro em tabela nunca é descartada.
4. `pipeline::run_translation` pula os nomes internos, consulta a memória de
   tradução e manda o resto ao provedor em lotes. Um lote é de um conjunto de
   caracteres só (`Charset`: sem restrição, ASCII ou uma tabela do projeto):
   o pedido leva a lista de caracteres e, em texto de tabela, o teto de cada
   frase. Depois de 2 lotes seguidos com erro a rodada para e devolve o
   motivo (`TranslateSummary.stopped`). `estimate_translation` monta os
   mesmos lotes sem chamar ninguém.
5. `validate.rs` confere cada tradução.
6. `reinsert.rs` valida de novo e aplica conforme `ErrorPolicy`: `Block`
   (um erro bloqueia tudo; é o da função antiga `reinsert_project`), `Skip`
   (a frase com erro fica no original; é o padrão da tela) ou `Force`. Grava
   em `working/` de forma atômica, relê do disco e roda `verify`.
7. `patch::export_patch` gera IPS ou BPS e confere que o patch reproduz a
   cópia de trabalho.

**Provedores** (`provider.rs` e `providers/`). O trait `TranslationProvider`
tem `translate_batch`, `health_check` e `usage` (o que as respostas declararam
de consumo). `ollama` e `openai_compat` falam HTTP; `anthropic` fala com a
API da Anthropic por HTTP direto (não há SDK oficial para Rust) e lê
`stop_reason` antes do conteúdo; `codex_cli` chama o `codex exec` do usuário
com as ferramentas do agente desligadas, e pode usar um `CODEX_HOME` próprio
(a "conta só do app").

**Texto por tabela** (`tbl.rs`). O `.tbl` usado no scan fica guardado em
`<projeto>/tables/`; validação, tradução e reinserção leem dali.

**Reinserção no lugar e ponteiros** (`adapters/inplace.rs`,
`adapters/pointers.rs`). `plan_in_place` confere que os bytes ainda são os da
extração e recusa duas traduções nos mesmos bytes. `find_pointer_tables` acha
tabelas; `relocate` grava o texto novo e reaponta. Onde o texto novo vai é
decisão de cada plataforma.

**Descoberta** (`relative.rs`, `xray.rs`, `lz77.rs`). Busca relativa, raio-X e
o LZ77 tipo 0x10 do BIOS do GBA e do DS, só para leitura.

**Dados.** Cada projeto tem um `translations.sqlite`. A memória de tradução
global e o `settings.toml` ficam na pasta de configuração do app; as chaves
de API vão para o cofre do sistema (uma entrada por provedor).

**Acrescentado em 13/10:**

- `pipeline::run_translation` tem três fases: memória de tradução; provider em
  levas de `parallel` lotes (`call_together`, um `join_all` feito à mão, com um
  relógio de 150 ms que faz o cancelamento valer na hora); repergunta do que
  não coube. Frase repetida vai uma vez (`first_of` / `copies`).
- `scan::stray_ids` + `ProjectDb::mark_stray`, chamados por
  `adapters::extract_project`.
- `crates/core/src/settings.rs`: as configurações saíram do crate do Tauri
  para o core, para a CLI ler as mesmas.
- `apps/cli`: casca fina sobre o core, um comando por função.

---

## 5. Estrutura de arquivos

```
crates/core/src/
  adapter.rs        trait GameAdapter, limites de tamanho, relatórios
  adapters/
    mod.rs          registro dos adapters e extract_project
    inplace.rs      reinserção no lugar (plano de escritas)
    pointers.rs     tabelas de ponteiros e relocação
    iso9660.rs      sistema de arquivos de disco (PS1, PS2, PSP)
    cdrom.rs        EDC/ECC de setor de CD
    rtsf.rs         formato sintético de referência
    gba.rs nds.rs nes.rs snes.rs gamecube.rs ps1.rs ps2.rs psp.rs
    wii.rs wiiu.rs  só detecção
  detect.rs         inspeção (hash + probes)
  cue.rs            leitura de cue sheet
  scan.rs           scanner genérico, "parece texto", "é nome interno"
  tbl.rs            tabelas .tbl: ler, codificar, guardar no projeto
  relative.rs       busca relativa: descobre a tabela de caracteres
  xray.rs           raio-X: comprimido, denso, frases com ponteiro
  lz77.rs           LZ77 tipo 0x10 do BIOS do GBA/DS
  project.rs        projeto .rtsproj
  db.rs             SQLite do projeto e memória global
  provider.rs       trait do provedor, prompt, leitura da resposta, Usage
  providers/        ollama.rs, openai_compat.rs, anthropic.rs, codex_cli.rs
  pipeline.rs       tradução: lotes, parada com motivo, estimativa de gasto
  validate.rs       validação e status das entradas
  reinsert.rs       reinserção e ErrorPolicy
  patch.rs          IPS e BPS
  fileio.rs         leitura por mapeamento em memória
  export.rs hash.rs types.rs error.rs
  synth.rs          arquivos sintéticos de teste (nenhum byte de jogo real)
  settings.rs       configurações, chaves de API e build_provider (veio do app em 13/10)
crates/core/tests/  25 arquivos de teste de integração
crates/core/examples/gen_fixtures.rs   grava 21 arquivos sintéticos em disco
apps/cli/           CLI `romtranslate` (13/10): src/main.rs e tests/cli.rs
apps/desktop/src-tauri/src/
  lib.rs            os 30 comandos expostos à interface
apps/desktop/src/
  App.tsx           tela inicial: escolher ou arrastar, inspecionar, criar ou abrir projeto
  ProjectView.tsx   a tela do projeto inteira
  i18n.ts           textos em pt-BR e en-US (um teste exige as mesmas chaves)
  types.ts util.ts  espelho dos tipos do core, tabela de preços, utilitários
.github/workflows/  ci.yml (3 jobs) e release.yml (4 plataformas)
docs/               SPEC.md (produto), ADAPTERS.md (guia), assets/
.ctx/               STATE.md, DECISIONS.md, HANDOFF.md, TOOL-LOG.md
```

**Acrescentado em 13/10:**

```
apps/cli/                 CLI `romtranslate` (src/main.rs, tests/cli.rs)
apps/desktop/.preview/    simulador do backend Tauri com jogo inventado
apps/desktop/src/assets/logo.svg   fonte única do logo e dos ícones
crates/core/src/settings.rs        configurações (antes no crate do Tauri)
crates/core/tests/fast_translate.rs  paralelo, repergunta, repetida, cancelar
crates/core/tests/stray.rs           marcação das strings soltas no banco
site/                     site: index.html, en/, estilo.css, site.js,
                          versao.js, build-demo.sh, conferir.mjs, publicar.sh
docs/DETALHES.md          o texto técnico que era o README
README.md / README.pt-BR.md   apresentação em inglês e em português
.github/FUNDING.yml       botão de apoio
```

---

## 6. Como rodar, validar e publicar

### 6.1 Rodar

Pré-requisitos: Rust estável, Node 20 ou mais novo, pnpm e as dependências de
sistema do Tauri 2.

```bash
pnpm install
pnpm dev                                    # abre o app
cargo run -p romtranslate-cli -- --help     # a linha de comando
cargo run -p romtranslate-core --example gen_fixtures   # arquivos de teste
```

Ver a interface sem o backend Rust, com um jogo inventado:

```bash
pnpm --dir apps/desktop exec vite --port 1437
# abrir http://localhost:1437/.preview/index.html?auto=1
```

### 6.2 Validar (tudo precisa passar antes de qualquer commit)

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
pnpm lint && pnpm test && pnpm web:build
```

Os testes manuais (Ollama, cofre do sistema, Codex CLI, API da Anthropic)
seguem como no `docs/DETALHES.md`. Teste com o Codex gasta a cota do
mantenedor: diga antes quantas chamadas vai fazer.

### 6.3 Publicar uma versão

1. Suba o número em `Cargo.toml`, `apps/desktop/package.json` e
   `apps/desktop/src-tauri/tauri.conf.json`. Rode `cargo check --workspace`
   para atualizar o `Cargo.lock`.
2. Valide (6.2), faça o commit e envie.
3. Tag no SHA explícito: `git tag vX.Y.Z <sha> && git push origin vX.Y.Z`.
4. O workflow compila as quatro plataformas e abre um release em **rascunho**
   com 9 arquivos.
5. Acrescente as novidades no topo das notas, com o que NÃO foi conferido
   (`gh release edit vX.Y.Z --notes-file ...`).
6. Publique: `gh release edit vX.Y.Z --draft=false`.
7. Confira sem login que a página abre e que um instalador baixa.
8. **Site:** troque o número em `site/versao.js` e rode `site/publicar.sh`.

Publicar versão só com pedido escrito do mantenedor.

### 6.4 Publicar o site

```bash
./site/publicar.sh
```

Compila a demonstração para `site/demo/`, confere os links
(`node site/conferir.mjs`) e roda `wrangler pages deploy site --project-name
romtranslate-studio --branch main`. O `wrangler` precisa estar logado na conta
Crimson Dawn.

**Refazer uma versão que ainda não foi publicada.** Se a tag já existe mas o
release está em rascunho e ficou velho: apague o rascunho
(`gh release delete vX.Y.Z --yes`), apague a tag remota e a local
(`git push origin :refs/tags/vX.Y.Z` e `git tag -d vX.Y.Z`) e crie a tag de
novo no commit certo, sempre com o SHA explícito (`git tag vX.Y.Z <sha>`).
Foi o que aconteceu com a v0.1.3 em 12/10. Nunca faça isso com um release já
publicado.

**Conferir um instalador de Mac sem abrir o app.** `gh release download` do
`.dmg`, `hdiutil attach -nobrowse -readonly`, ler a versão com `PlistBuddy` e
procurar no binário (`strings`) o nome de um command novo. O frontend não
aparece assim: vai comprimido dentro do binário.

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

**7.15 O `pnpm dev` reinicia o app a cada arquivo salvo.** O `tauri dev` vigia
`apps/desktop/src-tauri/` e `crates/core/` (testes incluídos). Editar ali com
o app do usuário aberto fecha e reabre a janela dele, e ele perde o que não
tinha salvado na tela. Rodar `cargo` em paralelo ainda faz o app dele esperar
pela trava do diretório `target/`. Avise antes, ou compile num `target`
separado (`CARGO_TARGET_DIR`).

**7.16 O backend usa a configuração gravada, não a que está na tela.** Todo
command que monta o provedor lê o `settings.toml`. A tela grava antes de
testar, traduzir ou calcular gasto; um botão novo que esqueça disso vai agir
sobre o provedor anterior.

**7.17 Erro fora de vista parece travamento.** O aviso geral da tela do
projeto fica no topo da página; os painéis de tradução e de reinserção ficam
lá embaixo. Erro de uma ação tem que aparecer no painel dela.

**7.18 Medir com o provedor do usuário gasta a cota dele.** Em 12/10 as
medições com o Codex levaram a conta do ChatGPT ao limite de uso no meio do
dia, e a tradução parou de funcionar por horas. Antes de rodar amostra real,
diga quantas chamadas vai fazer.

**7.19 Várias sessões na mesma pasta.** Com mais de um agente escrevendo no
mesmo checkout: `git add` só de caminhos explícitos; se um arquivo tem trecho
de outra sessão ainda não commitado, monte o índice só com os seus
(`git hash-object -w` + `git update-index --cacheinfo`), exporte com
`git checkout-index -a --prefix=<pasta>/` e valide nessa cópia. O `pnpm`
recusa `node_modules` raiz em symlink: rode os binários de
`apps/desktop/node_modules/.bin`.

**7.20 O Codex ecoa o prompt no stderr.** O motivo de uma falha é a última
linha que começa com `ERROR`, não o fim da saída.

**7.21 A fonte do jogo não é o ASCII.** No jogo medido, `# $ % & * + / ; < = >
@ [ \ ] ^ _` são desenhados como letras japonesas, e o lote ASCII do app
libera todos. A lista de caracteres do pedido tem que sair do que o jogo usa.

**7.22 Mesmo número de caracteres não é a mesma largura.** Letra acentuada e
"a" têm 7 px, "i" tem 3. Só dá para medir lendo os glifos da fonte na ROM.

**7.23 Entry de tabela pode começar com um parâmetro de código de controle.**
Traduzir sem preservar esse prefixo troca o parâmetro. Conferir, mensagem a
mensagem, que a sequência de códigos de controle é igual antes e depois pega
isso; checksum e re-extração não pegam.

**7.24 Traduzir tudo o que a extração devolve corrompe o jogo.** Um quarto das
entries pendentes do jogo medido era dado que parece texto. Projeto traduzido
antes da v0.1.5 precisa de "Extração estruturada" de novo antes de reinserir.

**7.25 Cancelar sem retorno parece botão quebrado.** Toda ação que demora
precisa mostrar que foi pedida.

**7.26 Dois checkouts no mesmo `target/`** deixam em `target/debug/` o binário
do que compilou por último: `cargo run` de um pode rodar o código do outro.

**7.27 A pasta de rascunho da sessão some quando ela reinicia.** O que vale
guardar vai para o repo ou para uma pasta do mantenedor antes.

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
- Publicar versão, mover tag ou anunciar o projeto: só com pedido escrito do
  mantenedor.
- Nenhum nome, imagem, personagem ou texto de jogo comercial no repositório,
  no site ou em captura de tela. Demonstração é sempre com jogo inventado.

---

## 9. Referências

- `docs/SPEC.md`: especificação do produto e roteiro original de 8 etapas.
- `docs/ADAPTERS.md`: como escrever um adapter.
- `.ctx/DECISIONS.md`: 44 decisões, com contexto e consequência. As de 12/10
  explicam o REA, o Codex CLI, a escolha de conta, a política de reinserção,
  a parada da tradução e o Claude por chave de API.
- Fontes dos formatos: GBATEK (GBA e DS, inclusive o LZ77 do BIOS), código do
  Dolphin (GameCube e Wii), Cemu e decaf-emu (Wii U), `iso_fs.h` do kernel
  Linux (ISO 9660), `ecm.c` de Neill Corlett (EDC/ECC), psdevwiki (PSP),
  repositório da `tauri-action` (workflow de release).
- API da Anthropic: referência oficial das Messages, da contagem de tokens e
  da lista de modelos; página "Legal and compliance" da documentação do
  Claude Code para o que é permitido com a assinatura.
- Formato do banco de mensagens do jogo medido: decompilação pública do jogo
  (ferramenta de strings e `charmap`), usada só como referência de formato.
- `docs/DETALHES.md`: o que cada parte faz, a matriz completa e a CLI.

## Commits mais recentes (o mais novo primeiro)

```
c2cec40 ctx: site no log e fim de linha das decisoes
3736a78 docs: link do site na pagina do projeto; ctx: site no ar
2cd0f17 feat(site): site de divulgacao com a demonstracao ao vivo, em portugues e ingles
34a2dda ctx: v0.1.5 publicada e conferida sem login
005dc16 ctx: correcoes do primeiro uso da v0.1.4 pela tela e a v0.1.5 a publicar
bca2e31 chore: v0.1.5
be565d2 fix(traducao): cancelar na hora, frase repetida uma vez so e o que esta fora das regioes de texto sai da fila
2c3893a ctx: v0.1.4 publicada, conferida sem login e instalada
f33b191 chore: v0.1.4
8d2a70a ctx: traducao em paralelo com repergunta, visual novo, pagina do projeto e o mapa do site
70d62f0 docs: captura do visual novo na pagina e simulador do backend pra ver a interface sem Rust
ef34b1c feat(ui): identidade nova de adesivo — paleta de doce, logo do cartucho e icones do app
2a6cbd0 docs: pagina do projeto em ingles e portugues, com o que foi medido e apoio por Pix
4807835 ctx: flags do translate na CLI
ad3493a feat(cli): translate com --parallel e --shorten-rounds
7c209be feat(traducao): 25 frases por chamada por padrao
52807c4 ctx: CLI do core — CI verde no Linux
6a857f4 ctx: CLI do core no estado, nas decisoes e no log
aed3448 feat(cli): CLI do core — o pipeline inteiro sem a janela
1d6863a feat(traducao): lotes em paralelo e repergunta do que nao coube no espaco da frase
0aae7b5 ctx: primeiro jogo inteiro pelo processo do app — resultado, 11 achados e a ordem de trabalho
c9c3335 ctx: v0.1.3 publicada e handoff 3
```

---
*Gerado por claude (opus-5.5) em 13/10/2026. Próxima ferramenta: leia
`.ctx/STATE.md` antes de editar e `.ctx/TOOL-LOG.md` para o histórico.*
