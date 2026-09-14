---
projeto: romtranslate-studio
handoff_gerado: 2026-09-14
gerado_por: claude
motivo: fim-de-feature
---

# HANDOFF.md — RomTranslate Studio

## 0. TL;DR
| Entregável | Estado | Path/URL | Próximo passo |
|---|---|---|---|
| Workspace Cargo+pnpm | pronto | raiz do repo | — |
| Detecção GBA/NES/SNES | pronto (Experimental, detect-only) | `crates/core/src/adapters/` | mais evidências conforme surgirem casos reais |
| Projeto `.rtsproj` | pronto (criar+carregar) | `crates/core/src/project.rs` | UI de "Abrir projeto" |
| App desktop | pronto (wizard mínimo) | `apps/desktop/` | Sprint 2: view de strings |
| CI | pronto | `.github/workflows/ci.yml` | job de build Tauri por SO |
| Sprint 2 (extração) | não começou | spec §10, §25 | scanners ASCII/UTF + .tbl |

## 1. Arquitetura técnica
- `romtranslate-core` (Rust, SEM Tauri): `detect::inspect(path)` → `InspectionReport { size, sha256, results, best }`. Roda todos os adapters de `adapters::all()`; cada `GameAdapter::probe(&GameInput)` devolve `ProbeResult { confidence 0-1, evidence, support_level }`. `best` = maior confiança ≥ 0.5.
- `GameInput` carrega só os primeiros 128 KiB (`PROBE_HEAD_LEN`) — cobre headers de NES/GBA/SNES incl. copier header. Limite de arquivo: 512 MiB (`MAX_FILE_SIZE`, vira config nas plataformas de disco).
- `project::create_project` grava `project.json` (escrita atômica tmp+rename) + subdirs `cache/extracted/working/exports`; recusa sobrescrever; NUNCA copia/modifica a ROM (guarda path+sha256).
- Shell Tauri: 2 commands (`inspect_file`, `create_project`) em `spawn_blocking`, serde `rename_all=camelCase` espelhado em `apps/desktop/src/types.ts`.
- UI: React, uma tela com 4 estados (home → inspecting → report → created). Strings SÓ via `t()` de `i18n.ts` (pt-BR/en-US; teste de paridade de chaves).

## 2. Estrutura de arquivos
```
crates/core/src/       adapter.rs (trait+GameInput) · adapters/{gba,nes,snes}.rs
                       detect.rs · hash.rs · project.rs · synth.rs · types.rs · error.rs
crates/core/tests/     pipeline.rs (integração: detecção, no-match, truncados, projeto)
crates/core/examples/  gen_fixtures.rs → fixtures/generated/ (gitignored)
apps/desktop/src/      App.tsx · i18n.ts · types.ts · util.ts · *.test.ts
apps/desktop/src-tauri/ lib.rs (commands) · tauri.conf.json · capabilities/
docs/SPEC.md           spec master completa (35 seções) — LER antes de sprint novo
```

## 3. Como validar
```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
pnpm lint && pnpm test && pnpm web:build
pnpm dev   # abre o app; teste manual com fixtures/generated/
```

## 4. Armadilhas conhecidas
- Probes NUNCA panicam: todo acesso a `head` com bounds check; teste `truncated_inputs_never_panic` cobre isso — mantenha ao adicionar adapter.
- Não embutir bytes do logo Nintendo (nem de nenhum jogo) — ver DECISIONS.md.
- `pnpm dev` na raiz roda via filter; o app precisa do vite em :1420 (config fixa).
- Máquina de casa não tinha Rust/pnpm — foram instalados 14/09; a do trabalho provavelmente também não tem (rustup + npm i -g pnpm).
