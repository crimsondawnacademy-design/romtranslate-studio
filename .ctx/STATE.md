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
App desktop open source (Tauri 2 + React + Rust) pra tradução de ROMs com IA, local-first, patch-first. Spec em `docs/SPEC.md`. Sprints 0–3 PRONTOS: detecção GBA/NES/SNES, projeto `.rtsproj`, extração de strings, e agora TRADUÇÃO (Ollama + OpenAI-compatible, TM + glossário em SQLite, batching/retry/cancel/progresso) — validada com Ollama REAL (llama3.2:3b traduziu preservando `{0}` e glossário). 42 testes. Próximo: Sprint 4 (editor + validação).

## Feito nesta sessão
- [2026-09-14] claude/fable-5 — Sprints 0+1+2 (ver TOOL-LOG) e Sprint 3 completo: trait `TranslationProvider` (async_trait) + providers Ollama (/api/chat, format=json) e OpenAI-compatible (/chat/completions), prompt spec §12 com glossário relevante por batch, parser tolerante (objeto/array/fences/<think>), `ProjectDb` SQLite (entries/tm/glossary/provider_runs, upsert preserva traduções em re-scan), pipeline com TM-first + batches sequenciais + retry backoff + cancel + progresso, settings.toml + secrets.json 0600 com guarda allow_remote_translation, 6 commands Tauri novos + evento translation-progress, UI: painel de tradução (provider/modelo/teste/progresso/cancelar), coluna tradução + status na tabela, glossário CRUD. Teste real com Ollama local passou (3/3, glossário respeitado).

## Próximos passos
1. [P1] Sprint 4 — editor/validação: edição manual de tradução na tabela, validador de placeholders/control codes, checagem de limite de bytes no encoding destino, filtros por status, marcar revisado (spec §14, §25).
2. [P2] Batches concorrentes com limite configurável se API remota virar gargalo (hoje sequencial).
3. [P2] TM global cross-projeto (hoje a TM vive no sqlite de cada projeto).
4. [P2] API key em keychain nativo (hoje secrets.json 0600 no config dir, fora do repo/projeto).
5. [P2] CI: job de build Tauri por SO; screenshot real no README.

## Bloqueios / Pendências
- [ ] Nenhum bloqueio. Repo PRIVADO na conta crimsondawnacademy-design; tornar público quando Rhuan decidir.

## Caminhos críticos
- Providers: `crates/core/src/provider.rs` (trait+prompt+parse) + `crates/core/src/providers/{ollama,openai_compat}.rs`
- Pipeline: `crates/core/src/pipeline.rs` (run_translation: TM→batches→retry→cancel)
- DB do projeto: `crates/core/src/db.rs` (`<proj>.rtsproj/translations.sqlite`)
- Settings do app: `apps/desktop/src-tauri/src/settings.rs` (settings.toml + secrets.json no app_config_dir)
- Commands: `apps/desktop/src-tauri/src/lib.rs` · UI do projeto: `apps/desktop/src/ProjectView.tsx`
- Teste real Ollama: `cargo test -p romtranslate-core --test translate -- --ignored --nocapture`
- Validação: ver bloco em `CLAUDE.md` do repo

## Glossário do projeto
- **provider**: implementação de `TranslationProvider` (async); Ollama local ou endpoint OpenAI-compatible; modelo NUNCA hardcoded
- **TM**: translation memory por projeto; chave = texto normalizado (trim+collapse, case preservado) + par de idiomas
- **glossário**: termos por projeto injetados no prompt SÓ quando aparecem no batch; `no_translate` = manter original
- **allow_remote_translation**: guarda de privacidade — endpoint fora de localhost exige opt-in explícito
- **scanner (Camada A)**: descoberta genérica em `scan.rs`; NÃO garante reinserção segura (Camada B, Sprint 5)
- **.rtsproj**: diretório do usuário (project.json + translations.sqlite + subdirs); NUNCA contém cópia da ROM
- **synth**: fixtures sintéticas — proibido byte de jogo real no repo
