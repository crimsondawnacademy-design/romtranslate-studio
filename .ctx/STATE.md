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
App desktop open source (Tauri 2 + React + Rust) pra tradução de ROMs com IA, local-first, patch-first. Spec completa em `docs/SPEC.md` (35 seções, roadmap de 8 sprints). Sprints 0 e 1 PRONTOS: workspace, detecção GBA/NES/SNES com evidências, SHA-256, projeto `.rtsproj`, 20 testes, CI. Próximo: Sprint 2 (extração de texto).

## Feito nesta sessão
- [2026-09-14] claude/fable-5 — Bootstrap completo: Cargo+pnpm workspace, crate `romtranslate-core` (adapters GBA/NES/SNES com probe+confiança+evidência, hash streaming, projeto .rtsproj com escrita atômica, fixtures sintéticas), app Tauri 2 (commands `inspect_file`/`create_project`, dialog plugin, UI dark pt-BR/en-US com i18n por message ID), 14 testes Rust + 6 vitest, clippy -D warnings limpo, CI GitHub Actions, docs (README/CONTRIBUTING/SECURITY/LICENSE/CLAUDE.md). Instalado rustup + pnpm nesta máquina (não existiam).

## Próximos passos
1. [P1] Sprint 2 — framework de extração de texto: scanners ASCII/UTF-8/UTF-16, loader de tabela `.tbl`, view de strings na UI, export JSON/CSV (spec §10 e §25).
2. [P2] Tela "Abrir projeto" (load_project já existe no core; UI só cria).
3. [P2] CI: job de build Tauri por SO quando for distribuir binário.
4. [P2] Screenshot real no README (placeholder hoje).

## Bloqueios / Pendências
- [ ] Nenhum bloqueio. Nota: repo criado PRIVADO na conta crimsondawnacademy-design; tornar público quando Rhuan decidir (é projeto pra ser OSS).

## Caminhos críticos
- Entrypoint core: `crates/core/src/lib.rs` (detect.rs = pipeline, adapter.rs = trait)
- Adapters: `crates/core/src/adapters/{gba,nes,snes}.rs`
- Commands Tauri: `apps/desktop/src-tauri/src/lib.rs`
- UI: `apps/desktop/src/App.tsx`, i18n em `apps/desktop/src/i18n.ts`
- Fixtures: `cargo run -p romtranslate-core --example gen_fixtures` → `fixtures/generated/`
- Validação: ver bloco em `CLAUDE.md` do repo

## Glossário do projeto
- **adapter**: implementação de `GameAdapter` por plataforma; declara capabilities honestas (hoje todos detect-only/Experimental)
- **probe**: detecção com confidence 0–1 + evidence; nunca panica, bounds check sempre
- **.rtsproj**: diretório de projeto do usuário (project.json + cache/extracted/working/exports); NUNCA contém cópia da ROM
- **synth**: fixtures sintéticas em `crates/core/src/synth.rs` — proibido byte de jogo real no repo
