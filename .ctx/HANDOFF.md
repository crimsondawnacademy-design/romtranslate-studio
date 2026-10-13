---
projeto: romtranslate-studio
handoff_gerado: 2026-10-12
gerado_por: claude / opus-5.5
motivo: fim-de-sessao-longa
branch: main
ultimo_commit_de_codigo: bb58981 (2026-10-12)
versao_publicada: v0.1.3
numero: 3
---

# HANDOFF 3 — RomTranslate Studio

Pra quem pega o projeto frio: leia as seções **1 a 3**. O resto é consulta.
O estado vivo fica em `.ctx/STATE.md`; o porquê de cada decisão, em
`.ctx/DECISIONS.md`; a especificação do produto, em `docs/SPEC.md`.

Este é o terceiro handoff. O segundo (12/10 de manhã) descrevia a v0.1.1; num
dia só o projeto ganhou duas versões e passou pelo primeiro teste em jogo
real. O documento foi refeito a partir do repositório, não de memória.

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
| Versão publicada | https://github.com/crimsondawnacademy-design/romtranslate-studio/releases/tag/v0.1.3 (12/10/2026, tag no commit `e2c7543`) |
| Stack | Tauri 2, React 19, Vite 8, TypeScript (pnpm); Rust em workspace |
| Tamanho | 11.908 linhas no core, 6.816 de testes, 933 na ponte Tauri, 2.842 de frontend |
| Testes | 221 em Rust (215 rodam; 6 são manuais) e 9 no frontend |
| CI | jobs `rust`, `desktop` e `frontend`, verdes no `e2c7543` |

A versão publicada é marcada como pré-lançamento (alpha). Por isso o endereço
`/releases/latest` **não** leva a ela: para divulgar, use o link da tag.

```bash
git clone https://github.com/crimsondawnacademy-design/romtranslate-studio.git
```

---

## 2. Estado atual (12 de outubro de 2026, fim do dia)

O ciclo completo funciona de ponta a ponta, e em 12/10 foi exercitado pela
primeira vez num jogo real (seção 2.2).

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

### 2.1 O caminho que a pessoa faz no app

1. Arrasta o jogo para a janela (ou escolhe o arquivo). Se já existe projeto
   para aquele arquivo, o app entra nele.
2. **Extração estruturada** (botão principal): guarda o espaço de cada frase,
   descarta o que não parece texto, tira do projeto o lixo antigo sem
   tradução e marca os nomes internos do jogo, que ficam fora da tradução.
3. Escolhe o provedor e clica em **Traduzir pendentes**. Provedores:
   - Ollama (local);
   - qualquer endpoint compatível com a API da OpenAI, com chave;
   - Claude pela API da Anthropic, com chave;
   - ChatGPT pelo Codex CLI (assinatura, sem chave), usando a conta do Codex
     do computador ou um login só do app.
   Provedor externo exige a permissão de tradução remota; a tela pergunta na
   hora. Se o provedor para de responder, a rodada para e mostra o motivo.
   Com chave paga, "Calcular gasto" mostra chamadas, tokens e valor antes, e
   a tela mostra o que foi cobrado depois.
4. **Reinserir na cópia de trabalho**: a tradução com erro fica de fora (a
   frase segue no original) e o resto entra. A tela mostra o caminho do jogo
   traduzido e um botão que abre a pasta.
5. **Exportar patch** para compartilhar.

Para jogo com tabela de caracteres própria (NES, SNES, Game Boy), a **busca
relativa** descobre a tabela a partir de uma palavra do jogo e o texto volta
pelo mesmo `.tbl`. O **Raio-X** mostra o que o app não lê: blocos LZ77 e as
frases dentro deles, quanto do arquivo é denso, quantas frases podem crescer.

### 2.2 O que o teste em jogo real mostrou

Dump do mantenedor de um jogo de GBA de 16 MiB em inglês, fora do
repositório, pelas funções do core que os botões chamam. Só números:

| Medida | Resultado |
|---|---|
| Entradas extraídas | 17.936 (169.226 sequências descartadas como não texto) |
| Nomes internos fora da fila | 2.326; ficam 15.610 para traduzir |
| Frases com ponteiro em tabela | 1.590, quase todas nomes internos; nenhuma fala |
| Blocos LZ77 | 1.599, 279 com texto, cerca de 10 mil frases dentro |
| Tradução de 30 falas pelo ChatGPT (lote de 10) | 30 de 30, em 40 s |
| Passam na validação sem intervenção | 26 a 27 de 30 depois do aviso de ASCII (eram 16) |
| Reinserção das válidas | arquivo do mesmo tamanho, nenhum byte alterado fora das frases, checksum válido |

O que isso diz: a mecânica funciona em jogo real. O diálogo é ASCII legível,
mas cada entrada é **uma linha** da caixa de texto, e todo o diálogo é gravado
no lugar. **Ninguém viu o jogo rodando com a tradução.**

---

## 3. Pendências — por prioridade

### 3.1 O que impede traduzir um jogo inteiro hoje

1. **Volume contra o limite da assinatura.** 15.610 linhas são 1.561 chamadas
   com 10 frases cada. A assinatura do ChatGPT não cobre isso numa janela de
   limite: a rodada para no meio e a pessoa retoma quando a conta volta (o
   que foi traduzido fica salvo). Falta medir quantas frases por chamada o
   modelo aguenta sem perder qualidade nem estourar o espaço; só o lote de 10
   foi medido. Ver armadilha 7.18 antes de medir.
2. **Ver o resultado num emulador.** Nunca foi feito. Sem isso não se sabe se
   o jogo aceita a sobra preenchida, se o texto cabe na caixa, se trava.
3. **Acento.** O texto ASCII sai sem acento. Acento de verdade depende de o
   jogo ter as letras na fonte e de um `.tbl` com os códigos delas.
4. **Linha por linha.** A IA traduz pedaço de frase sem ver o resto, e o
   texto não passa de uma linha para a outra.
5. **Texto comprimido.** O Raio-X conta; o app não extrai nem regrava.

### 3.2 O que foi construído e ainda não foi visto funcionando de verdade

- **Claude por chave de API**: nenhuma chamada real foi feita (não havia
  chave). O formato veio da referência da API e tem teste sem rede. O
  primeiro teste real é colar uma chave e clicar em "Testar conexão", que não
  gasta token; há dois testes manuais em `crates/core/tests/anthropic.rs`.
- **Cálculo de gasto**: conferido com backend simulado. A calibragem pela
  contagem de tokens do Claude depende do item acima.
- **Tradução longa pelo app**: a rodada mais longa feita de verdade tinha 80
  linhas, pelo core. Pelo app, só se viu a parada por limite de conta.
- **Telas**: o mantenedor usou no app a detecção, a criação de projeto, a
  extração com filtro, a permissão remota, o login do ChatGPT só do app e a
  parada com motivo. Arrastar e soltar, "Mostrar o arquivo", a busca
  relativa, o Raio-X e a reinserção pelo app só foram conferidos com backend
  simulado.
- **Windows e Linux**: os instaladores compilam no CI; ninguém abriu.

### 3.3 Decisões que são do mantenedor

- **Claude Code pela assinatura.** Não foi construído. Os termos do Claude
  Code permitem a pessoa usar o próprio binário, sem modificação, com a
  própria assinatura; rodar o Claude Code dentro de um produto exige aceitar
  os Termos Comerciais da Anthropic. Sem esse aceite, o código não entra.
  Detalhe em `.ctx/DECISIONS.md` (12/10).
- **Login do OpenRouter.** É o único login por navegador feito para app de
  terceiros, com modelos grátis e pagos. Foi pesquisado e proposto; falta a
  decisão de construir.
- **Ghidra + Java** (uns 2 GB) para usar o REA como bancada de engenharia
  reversa num jogo específico.
- **Divulgação** (r/romhacking, ROMhacking.net). Vale só depois de ver uma
  tradução rodando num emulador.

### 3.4 Prioridade 2 e 3

- Tabela de ponteiros indicada pelo usuário (estilo Atlas): o caminho para
  NES e SNES e para tabelas que a detecção recusa.
- Extrair e regravar texto de dentro de bloco LZ77; reconhecer o LZ11.
- Lotes de tradução em paralelo.
- Shift-JIS; edição de fonte; Wii e Wii U (cifrados, sem chaves por decisão).
- Mostrar na tela o endereço do login quando o navegador não abre.
- `apps/desktop/README.md` ainda é o do template; a versão do Rust não é
  fixada (um lint novo pode quebrar o CI sem ninguém mexer no código).

A lista completa, com o histórico de cada item, está em `.ctx/STATE.md`.

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
crates/core/tests/  25 arquivos de teste de integração
crates/core/examples/gen_fixtures.rs   grava 21 arquivos sintéticos em disco
apps/desktop/src-tauri/src/
  lib.rs            os 30 comandos expostos à interface
  settings.rs       configurações e chaves de API
apps/desktop/src/
  App.tsx           tela inicial: escolher ou arrastar, inspecionar, criar ou abrir projeto
  ProjectView.tsx   a tela do projeto inteira
  i18n.ts           textos em pt-BR e en-US (um teste exige as mesmas chaves)
  types.ts util.ts  espelho dos tipos do core, tabela de preços, utilitários
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

Seis testes são manuais e ficam fora do CI:

```bash
cargo test -p romtranslate-core --test translate -- --ignored --nocapture  # exige Ollama rodando
cargo test -p romtranslate-desktop -- --ignored                            # grava no cofre do sistema
cargo test -p romtranslate-core --test codex_cli -- --ignored --nocapture  # exige Codex CLI logado; gasta cota
ANTHROPIC_API_KEY=... cargo test -p romtranslate-core --test anthropic -- --ignored --nocapture  # 1 teste não cobra, 1 cobra
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

## Commits mais recentes (o mais novo primeiro)

```
e2c7543 ctx: Claude por chave de API, calculo de gasto e a decisao sobre o Claude Code
bb58981 feat(traducao): Claude por chave de API e calculo de gasto em tokens
7291159 ctx: auditoria do caminho IA -> jogo no estado, nas decisoes e no log
61a4a1b fix(fluxo): da traducao ao jogo sem beco sem saida — permissao, limite da conta e frases com erro
7596d98 ctx: escolha da conta do ChatGPT no estado, nas decisoes e no log
4270211 feat(traducao): escolher a conta do ChatGPT — a do computador ou uma so do app
c270499 ctx: texto ASCII sem barreira de acento e nomes internos fora da fila, com a medicao em jogo real
b0da48f feat(traducao): texto ASCII leva a lista de caracteres e troca acento com aviso; nome interno fica fora da fila
d06fb04 ctx: primeiro teste de ponta a ponta em jogo real
c83cc80 chore: v0.1.3
5540aa2 fix(ui): Testar conexao e Traduzir usam o provider da tela; erro aparece junto dos botoes
173558f ctx: criar projeto segue no projeto existente
9c29365 fix(ui): "Criar projeto" segue no projeto que ja existe pro mesmo arquivo
3529822 ctx: v0.1.2 publicada e conferida sem login
```

---
*Gerado por claude (opus-5.5) em 12/10/2026. Próxima ferramenta: leia
`.ctx/STATE.md` antes de editar e `.ctx/TOOL-LOG.md` para o histórico.*
