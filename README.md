# RomTranslate Studio

> Local-first, AI-assisted game translation toolkit.

Ferramenta desktop open source para extrair, traduzir, revisar e reaplicar textos em
jogos compatíveis, com IA local (Ollama) ou APIs configuráveis, gerando **patches**
em vez de cópias modificadas.

**Status: alpha (Sprint 1).** Hoje o app identifica plataforma (GBA, NES, SNES),
calcula SHA-256 e cria projetos locais `.rtsproj`. Extração e tradução vêm nos
próximos sprints.

![screenshot placeholder](docs/screenshot-placeholder.png)

## O que funciona hoje

- Seleção local de arquivo (nada é enviado a lugar nenhum);
- Detecção de plataforma com confiança e evidências:
  - **GBA** — byte fixo + header checksum (GBATEK);
  - **NES** — magic iNES/NES 2.0 + consistência de tamanho PRG/CHR;
  - **SNES** — pontuação de header interno LoROM/HiROM, com suporte a copier header (.smc);
- SHA-256 streaming do arquivo;
- Criação de projeto local `NomeDoJogo.rtsproj/` (path + hash da origem; o original
  **nunca** é copiado nem modificado);
- Fixtures sintéticas para teste — nenhuma ROM comercial no repositório.

## Níveis de suporte

Jogos usam engines, compressões e tabelas diferentes — **não prometemos suporte
universal**. Cada adapter declara: `Full`, `Partial`, `ExtractOnly`, `Experimental`
ou `Unsupported`. Os adapters atuais são somente-detecção (`Experimental`).

## Rodando

Pré-requisitos: [Rust](https://rustup.rs) estável, Node 20+, [pnpm](https://pnpm.io),
e as [dependências do Tauri 2](https://tauri.app/start/prerequisites/) do seu SO.

```bash
pnpm install
pnpm dev          # abre o app (tauri dev)
```

Testes e checks:

```bash
cargo test --workspace
cargo fmt --check && cargo clippy --all-targets -- -D warnings
pnpm test && pnpm lint && pnpm web:build
```

Fixtures sintéticas para testar a UI sem ROM real:

```bash
cargo run -p romtranslate-core --example gen_fixtures
# gera fixtures/generated/*.{gba,nes,sfc,smc}
```

## Arquitetura

```
apps/desktop/        # Tauri 2 + React + TS (shell fino; não contém lógica de domínio)
crates/core/         # detecção, hashing, projeto — sem dependência de Tauri
  src/adapter.rs     # trait GameAdapter + GameInput (probing com bounds check)
  src/adapters/      # gba, nes, snes
  src/detect.rs      # pipeline de inspeção
  src/project.rs     # projeto .rtsproj (project.json, escrita atômica)
  src/synth.rs       # fixtures sintéticas
```

Providers de tradução (Ollama, OpenAI-compatible), memória de tradução, glossário,
validação, reinserção e export de patch (IPS/BPS) entram nos Sprints 2–6 — veja
[docs/SPEC.md](docs/SPEC.md).

## Aviso legal

RomTranslate Studio é uma ferramenta de tradução e modificação para arquivos de
jogos obtidos legalmente pelo próprio usuário. O projeto **não distribui** jogos,
BIOS, keys, firmware ou conteúdo proprietário, e não inclui mecanismos para
baixá-los. Incentivamos a distribuição de **patches** em vez de imagens completas
modificadas e o respeito às leis e licenças aplicáveis.

## Contribuindo

Veja [CONTRIBUTING.md](CONTRIBUTING.md). Adapters de plataforma são o melhor lugar
para contribuir — a interface está em `crates/core/src/adapter.rs`.

Licença: [MIT](LICENSE).
