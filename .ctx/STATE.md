---
projeto: romtranslate-studio
status: em-andamento
ultima_edicao: 2026-09-14
ultima_ferramenta: claude
ultima_modelo: claude-fable-5
ultima_maquina: Mac mini M1 (casa)
branch: main
---

# STATE.md — RomTranslate Studio

> Arquivo leve (~50 linhas), sempre atualizado no fim de cada sessão/feature.
> Lido por Claude Code, Codex e ZCode antes de qualquer trabalho.
> Política CCP — não delete seções, só atualize.

## TL;DR
App desktop open source (Tauri 2 + React + Rust) pra tradução de ROMs com IA, local-first, patch-first. Spec completa em `docs/SPEC.md`. Sprints 0, 1 e 2 PRONTOS: detecção GBA/NES/SNES, projeto `.rtsproj`, extração de strings (ASCII/UTF-8/UTF-16 LE-BE/tabela .tbl) com view na UI e export JSON/CSV. 31 testes. Próximo: Sprint 3 (tradução: providers Ollama/OpenAI-compatible + TM + glossário).

## Feito nesta sessão
- [2026-09-14] claude/fable-5 — Sprints 0+1 (bootstrap completo, ver TOOL-LOG) e Sprint 2: módulos `scan`/`tbl`/`export` no core (scanners determinísticos com região/min-chars/cap, tabela .tbl com longest-match, CSV RFC4180 + JSON), `open_project` com verificação de SHA-256 da origem (avisa se mudou/faltando), commands Tauri novos (`scan_file`, `export_entries`, `open_project`), tela de extração na UI (config, filtro, cap de render 500, export via save dialog) e "Abrir projeto" na home, strings plantadas na fixture GBA, +10 testes Rust +1 vitest. Regra do Rhuan registrada: projeto pessoal, nada vai pra VPS nem pro assistente interno.

## Próximos passos
1. [P1] Sprint 3 — tradução: trait `TranslationProvider`, providers Ollama + OpenAI-compatible (config, health check, batching, retries), translation memory + glossário em SQLite (spec §11-13, §25).
2. [P2] Persistir entries extraídas no projeto automaticamente (hoje o export é manual via dialog).
3. [P2] CI: job de build Tauri por SO quando for distribuir binário.
4. [P2] Screenshot real no README.

## Bloqueios / Pendências
- [ ] Nenhum bloqueio. Repo PRIVADO na conta crimsondawnacademy-design; tornar público quando Rhuan decidir.

## Caminhos críticos
- Entrypoint core: `crates/core/src/lib.rs` (detect = inspeção, scan = extração Camada A)
- Scanners: `crates/core/src/scan.rs` + `crates/core/src/tbl.rs` + `crates/core/src/export.rs`
- Adapters: `crates/core/src/adapters/{gba,nes,snes}.rs`
- Commands Tauri: `apps/desktop/src-tauri/src/lib.rs`
- UI: `apps/desktop/src/App.tsx`, i18n em `apps/desktop/src/i18n.ts` (t() com {params})
- Fixtures: `cargo run -p romtranslate-core --example gen_fixtures` → `fixtures/generated/`
- Validação: ver bloco em `CLAUDE.md` do repo

## Glossário do projeto
- **adapter**: implementação de `GameAdapter` por plataforma; declara capabilities honestas (hoje todos detect-only/Experimental)
- **probe**: detecção com confidence 0–1 + evidence; nunca panica, bounds check sempre
- **scanner (Camada A)**: descoberta genérica de strings em `scan.rs`; NÃO garante reinserção segura — isso é da Camada B (adapters estruturados, Sprint 5)
- **.tbl**: tabela de caracteres custom `HEX=texto`, longest-match greedy
- **.rtsproj**: diretório de projeto do usuário (project.json + cache/extracted/working/exports); NUNCA contém cópia da ROM
- **synth**: fixtures sintéticas em `crates/core/src/synth.rs` — proibido byte de jogo real no repo
