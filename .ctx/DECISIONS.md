# DECISIONS.md — RomTranslate Studio

> Append-only. Só cresce quando há decisão durável (arquitetura, stack, padrão).
> Não edite entradas antigas — se uma decisão foi revertida, adicione nova entrada marcando "[reverte YYYY-MM-DD]".
> Teste de sinal antes de adicionar: isto muda uma decisão futura? Se não, não guarde.

## [2026-09-14] Workspace enxuto: 1 crate core em vez dos 7 da spec

**Contexto:** a spec (docs/SPEC.md §4) sugere 7 crates (core, formats, translation, translation-memory, patching, project, plugin-api) + plugins/. No Sprint 0-1 só existe detecção/hash/projeto.

**Decisão:** um único `crates/core` com módulos internos (adapter, adapters/, detect, hash, project, synth, types). A própria spec autoriza começar menor preservando interfaces. Split em crates quando um domínio novo (translation, patching) tiver código de verdade.

**Alternativas consideradas:**
- 7 crates vazios desde já — boilerplate sem função, atrito pra navegar.

**Consequências:** o trait `GameAdapter` e os tipos já isolam o que virará `plugin-api`; mover módulo pra crate depois é mecânico.

**Driver:** claude (ponytail).

## [2026-09-14] Trait GameAdapter sync (spec pedia async)

**Contexto:** spec §8 define trait com async_trait. Probing é IO local de KBs, instantâneo; async_trait adicionaria dep + ruído em todos os adapters.

**Decisão:** trait sync. Comandos Tauri rodam em `spawn_blocking` (UI não trava). Revisitar no Sprint 3 se extração/tradução precisarem de IO async de verdade.

**Consequências:** migração futura é mudança mecânica de assinatura; adapters de comunidade escrevem menos boilerplate hoje.

**Driver:** claude (ponytail).

## [2026-09-14] Probe GBA sem embutir o logo Nintendo

**Contexto:** detecção canônica de GBA compara os 156 bytes do logo comprimido (0x04–0x9F). Embutir esses bytes no repo é distribuir material proprietário — viola o princípio §2.

**Decisão:** evidências alternativas: byte fixo 0x96 em 0xB2 + header checksum de 0xA0..=0xBC + pistas fracas (branch ARM, título ASCII). Fixtures sintéticas idem.

**Consequências:** ROM real com header corrompido pontua mais baixo que num detector por logo — aceitável, a UI mostra evidências e confiança.

**Driver:** claude.

## [2026-09-14] Fixtures geradas por código, não commitadas

**Contexto:** spec pede fixtures/. Binários em git incham histórico e convidam alguém a commitar ROM de verdade "só pra testar".

**Decisão:** geradores determinísticos em `synth.rs` (usados pelos testes em memória) + example `gen_fixtures` que materializa em `fixtures/generated/` (gitignored) pra teste manual da GUI.

**Consequências:** teste sempre reflete o gerador; zero binário no repo.

**Driver:** claude.

## [2026-09-14] SNES: fixture valida PAR checksum/complement, não a soma real

**Contexto:** o probe SNES pontua `complement ^ checksum == 0xFFFF` (barato e forte contra falso positivo). Recalcular a soma real do arquivo é trabalho da fase de reinserção (Sprint 5), não da detecção.

**Decisão:** probe e fixture cobrem só o par. Quando a reinserção recalcular checksums internos (spec §15), aí entra a soma canônica com testes próprios.

**Driver:** claude.
