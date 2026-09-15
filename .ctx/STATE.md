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
App desktop open source (Tauri 2 + React + Rust) pra tradução de ROMs com IA, local-first, patch-first. Spec em `docs/SPEC.md`. TODOS os sprints da spec (0–8) cobertos. Sprint 8: NDS completo (probe CRC-16, filesystem FNT/FAT, extração por arquivo ASCII+UTF-16LE com resource_path, reinserção in-place, CRC recalculado) + probes GameCube/Wii/WBFS honestos (Wii cifrado: sem extração por design). Helper in-place compartilhado GBA/NDS. 71 testes. Próximos: NES/SNES conservador, BPS (NDS real >16MiB), Wii U (formato a verificar), abrir o repo.

## Feito nesta sessão
- [2026-09-15] claude/fable-5 — Sprint 8 (containers): adapter NDS (`nds.rs`): probe por CRC-16 do header + campo do logo 0xCF56 (sem embutir o logo), parser FNT/FAT com bounds/anti-ciclo, list_resources no trait (+ResourceDescriptor), extração por arquivo (ASCII+UTF-16LE, resource_path preenchido, cap 20k), reinserção in-place com CRC recalculado; helper `adapters/inplace.rs` compartilhado (GBA refatorado, agora com suporte UTF-16); probes GameCube (magic 0x1C+título) e Wii (magic 0x18, WBFS, aviso de partições cifradas); MAX_FILE_SIZE de inspeção → 16 GiB, IN_MEMORY_MAX 512 MiB pra extract/reinsert; coluna Arquivo na tabela; matriz do README atualizada; Wii U fora (formato não verificado — sem chute). +7 testes. Antes: Sprint 7 (comunidade): docs/ADAPTERS.md (contrato, regras inegociáveis, campos de TextEntry p/ reinserção segura, testes obrigatórios, esqueleto), README reescrito pro estado atual com matriz de compatibilidade (spec §26), issue templates YAML (bug/adapter/compatibility com SHA-256 em vez de ROM/feature) + PR template com checklist, CONTRIBUTING aponta pro guia. Decisão mantida: sem plugin loading dinâmico — o trait documentado É o SDK inicial. Antes: Sprint 6 + GBA: `patch.rs` (IPS create/apply em Rust puro: RLE e truncate no apply, edge do offset 0x454F46, merge de gaps, limite 16 MiB com erro claro; round-trip interno obrigatório antes de exportar), `export_patch` gera exports/<stem>.<lang>.{ips,manifest.json,translations.csv} (manifest snake_case da spec §16), command+botão na UI. GBA ganhou extract_structured (scan ASCII com max_bytes = espaço original, ids iguais ao scanner → sem duplicata), apply_text in-place all-or-nothing (sanity anti-drift dos bytes, padding por terminated, header checksum recalculado — traduzir o título funciona) e verify. Capabilities patch=true (RTSF e GBA). +6 testes incl. fluxo GBA ponta a ponta e DoD do patch. [2026-09-14] Sprint 5: formato sintético RTSF + adapter `synthetic.rtsf` (probe 0.99, extract estruturado com max_bytes real, apply all-or-nothing com relocação de blob + update de pointer table + checksum recalculado, verify), trait GameAdapter ganhou extract_structured/apply_text/verify (default "não suportado"), `reinsert::reinsert_project` (§15: confere SHA-256 da origem, valida, bloqueia Error salvo allow_errors, grava working/ atômico, relê e verifica), commands extract_structured/reinsert_project, UI: botão de extração estruturada + painel de reinserção. 8 testes novos incl. round-trip completo e headers mentirosos. Antes: Sprint 4: módulo `validate` (tokens {..}/<..>/[..]/%x/\x em multiset, encoded_len por encoding, overflow vs max_bytes ou espaço original, vazias, anomalia 3x), `validate_project_db` ajusta statuses (Error↔Machine, Reviewed preservado), `apply_manual_translation` (edição grava TM + valida), `set_reviewed` exige validação limpa; commands update_entry/set_entry_reviewed/validate_project; UI: filtros por status+issue, painel editor com bytes live e issues, validação automática pós-tradução. Antes na mesma sessão: Sprints 0+1+2+3 (ver TOOL-LOG): trait `TranslationProvider` (async_trait) + providers Ollama (/api/chat, format=json) e OpenAI-compatible (/chat/completions), prompt spec §12 com glossário relevante por batch, parser tolerante (objeto/array/fences/<think>), `ProjectDb` SQLite (entries/tm/glossary/provider_runs, upsert preserva traduções em re-scan), pipeline com TM-first + batches sequenciais + retry backoff + cancel + progresso, settings.toml + secrets.json 0600 com guarda allow_remote_translation, 6 commands Tauri novos + evento translation-progress, UI: painel de tradução (provider/modelo/teste/progresso/cancelar), coluna tradução + status na tabela, glossário CRUD. Teste real com Ollama local passou (3/3, glossário respeitado).

## Próximos passos
1. [P1] BPS backend — NDS real passa de 16 MiB (limite do IPS); BPS também destrava truncamento.
2. [P1] Reinserção conservadora NES/SNES (helper in-place pronto; SNES exige recalcular checksum/complement do header interno).
3. [P2] Wii U probe (verificar formato WUD/WUX/RPX antes — sem chute); FST do GameCube (plaintext, listável); checkbox modo avançado; batches concorrentes; TM global; keychain; CI build Tauri por SO; screenshot no README; abrir o repo ao público.

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
