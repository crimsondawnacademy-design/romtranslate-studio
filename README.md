# RomTranslate Studio

> Local-first, AI-assisted game translation toolkit.

Ferramenta desktop open source para extrair, traduzir, revisar e reaplicar textos em
jogos compatíveis, com IA local (Ollama) ou APIs configuráveis, gerando **patches** (IPS/BPS)
em vez de cópias modificadas.

**Status: alpha.** O pipeline completo funciona de ponta a ponta:
detecção → extração → tradução com IA → validação/revisão → reinserção em
cópia de trabalho → **patch IPS** com manifest.

<!-- TODO: screenshot da tela do projeto quando a UI estabilizar -->

## O que funciona hoje

- **Detecção** de plataforma com confiança e evidências (GBA, NES, SNES e a
  fixture sintética RTSF);
- **Extração**: scanner genérico (ASCII, UTF-8, UTF-16 LE/BE, tabelas `.tbl`)
  para descoberta + extração estruturada com limites reais nos adapters que a
  suportam;
- **Tradução por IA**: Ollama (local, padrão) ou qualquer endpoint
  OpenAI-compatible; batching, retry, cancelamento, progresso; **translation
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
| Game Boy Advance | ✅ | ✅ | ⚠️ experimental (in-place) | ⚠️ experimental (in-place) | ✅ |
| Nintendo DS | ✅ (CRC do header) | ✅ | ⚠️ experimental (por arquivo do filesystem, ASCII + UTF-16) | ⚠️ experimental (in-place) | ✅ (BPS acima de 16 MiB) |
| NES | ✅ | ✅ | ⚠️ experimental (in-place; jogos com tabela própria: use `.tbl`) | ⚠️ experimental (in-place) | ✅ |
| Super Nintendo | ✅ | ✅ | ⚠️ experimental (in-place) | ⚠️ experimental (in-place, checksum interno recalculado) | ✅ |
| GameCube | ✅ (magic + FST) | — | ⚠️ experimental (por arquivo do filesystem FST) | ⚠️ experimental (in-place) | ✅ (BPS p/ ISO real) |
| Wii | ✅ (ISO e WBFS) | — | — | — | — |
| Wii U | ✅ (WUX e RPX/RPL) | — | — | — | — |
| PlayStation (PS1) | ✅ (ISO 9660 + SYSTEM.CNF) | ✅ | ⚠️ experimental (por arquivo; BIN raw por setor) | ⚠️ experimental (in-place; BIN raw com EDC/ECC regenerado)* | ✅ |
| PlayStation 2 | ✅ (BOOT2 no SYSTEM.CNF) | ✅ | ⚠️ experimental (por arquivo; imagens até 2 GiB) | ⚠️ experimental (in-place) | ✅ (BPS) |
| PSP | ✅ (UMD + PARAM.SFO com título) | ✅ | ⚠️ experimental (por arquivo) | ⚠️ experimental (in-place) | ✅ (BPS) |

\* BIN raw de PS1 (2352 bytes/setor): a reinserção regenera o EDC/ECC
(ECMA-130) de cada setor alterado, e o verify confere o EDC da imagem inteira.
Imagens multi-track (`.cue` com áudio) ainda não: use o track de dados.

O export escolhe o formato sozinho: **IPS** quando cabe (máxima
compatibilidade) e **BPS** para arquivos maiores ou que encolhem — o BPS ainda
valida por CRC-32 que o patch está sendo aplicado no arquivo certo. Partições
de disco Wii são cifradas: extração exigiria keys, que este projeto não inclui.

**"In-place" (GBA)**: cada string traduzida ocupa o espaço da original (mesmo
tamanho ou menor) — cobre menus e textos curtos de muitos jogos; textos com
ponteiros/compressão pedem adapter dedicado. O validador avisa o que não cabe
antes de qualquer escrita.

## Rodando

Pré-requisitos: [Rust](https://rustup.rs) estável, Node 20+, [pnpm](https://pnpm.io),
e as [dependências do Tauri 2](https://tauri.app/start/prerequisites/) do seu SO.
Para tradução local, [Ollama](https://ollama.com) com um modelo puxado
(ex.: `ollama pull llama3.2:3b`).

```bash
pnpm install
pnpm dev          # abre o app (tauri dev)
```

Fluxo no app: selecionar arquivo → criar projeto → **Extração estruturada** (ou
scan genérico) → configurar provider e **Traduzir** → revisar no editor →
**Reinserir na cópia de trabalho** → **Exportar patch (IPS)**. Os artefatos saem
em `SeuJogo.rtsproj/exports/`.

Fixtures sintéticas para testar sem ROM real:

```bash
cargo run -p romtranslate-core --example gen_fixtures
# gera fixtures/generated/*.{gba,nes,sfc,smc,rtsf}
```

Testes e checks:

```bash
cargo test --workspace
cargo fmt --check && cargo clippy --all-targets -- -D warnings
pnpm test && pnpm lint && pnpm web:build
# prova de fogo com Ollama rodando:
cargo test -p romtranslate-core --test translate -- --ignored --nocapture
```

## Arquitetura

```
apps/desktop/        # Tauri 2 + React + TS (shell fino; zero lógica de domínio)
crates/core/         # tudo testável sem UI
  src/adapter.rs     # trait GameAdapter (probe/extract/apply/verify)
  src/adapters/      # rtsf (referência completa), gba, nes, snes
  src/scan.rs        # scanner genérico (Camada A)
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
