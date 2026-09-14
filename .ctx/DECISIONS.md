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

## [2026-09-14] Projeto pessoal: nada na VPS da agência nem no assistente interno

**Contexto:** o RomTranslate Studio é projeto PESSOAL do Rhuan. A infra da empresa (VPS [ip interno removido], agentes internos, RAG da empresa) é da agência.

**Decisão:** nenhum deploy, automação, cron ou registro deste projeto vai para a VPS da empresa nem para o assistente interno. O projeto vive nas máquinas locais + repo privado GitHub. Se um dia precisar de automação agendada, é na infra pessoal (infra pessoal), nunca na da agência.

**Driver:** rhuan (14/09/2026).

## [2026-09-14] Scanner genérico fora do trait GameAdapter

**Contexto:** spec §8 põe `extract_text` no trait de adapter; spec §10 define a Camada A (scanner genérico) como ferramenta de descoberta que serve qualquer plataforma.

**Decisão:** a Camada A vive em `scan.rs` como função independente do adapter — a UI chama direto. `extract_text` estruturado entra no trait quando a Camada B nascer (fixture adapter do Sprint 5), onde extração de verdade depende de formato/ponteiros por plataforma.

**Consequências:** trait continua enxuto; scanner reutilizável em CLI futura sem adapter.

**Driver:** claude (ponytail).

## [2026-09-14] Shift-JIS adiado

**Contexto:** spec §10 lista Shift-JIS "quando apropriado". Exige tabela de conversão (crate encoding_rs).

**Decisão:** fora do Sprint 2. Entra com `encoding_rs` quando houver adapter/caso de uso japonês real; a arquitetura (enum `ScanEncoding` + `TextEncoding::ShiftJis` já existente) já reserva o lugar.

**Driver:** claude (ponytail).

## [2026-09-14] Sprint 3: um SQLite por projeto; TM global adiada

**Contexto:** spec §13 pede TM com "optional game scope" e glossário global-ou-projeto. O arquivo `translations.sqlite` dentro do `.rtsproj` (spec §20) já dá TM e glossário por projeto com zero infra extra.

**Decisão:** um banco por projeto. TM global cross-projeto (e glossário global) entram depois como segundo banco no config dir, consultado em cascata.

**Consequências:** retraduzir o mesmo jogo é grátis via TM; reaproveitar entre jogos ainda não.

**Driver:** claude (ponytail).

## [2026-09-14] Secrets: arquivo 0600 separado, keyring adiado

**Contexto:** spec §11 pede keychain "quando possível"; §18 exige secrets fora do config normal. Crate keyring adiciona dep nativa + prompts de sistema.

**Decisão:** API key em `secrets.json` (0600) no app_config_dir — fora do repo, fora do projeto, fora de logs; `settings.toml` nunca contém a chave. Upgrade pra keyring quando o app for distribuído.

**Driver:** claude (ponytail).

## [2026-09-14] Batches sequenciais; privacidade remota com opt-in

**Contexto:** spec §28 sugere batches concorrentes configuráveis; §18 default `allow_remote_translation=false`.

**Decisão:** pipeline sequencial (Ollama local não paraleliza de verdade; concorrência entra se API remota medir como gargalo). Endpoint OpenAI-compatible fora de localhost é BLOQUEADO até o usuário ativar "permitir tradução remota" — localhost (LM Studio, vLLM local) passa sempre.

**Driver:** claude (ponytail).

## [2026-09-14] Validação: tokens unificados; revisão exige validação limpa

**Contexto:** spec §14 separa placeholders/control codes/tags. Nos textos extraídos (printable), todos são padrões no texto: {..}, <..>, [..], %x, \x.

**Decisão:** um extrator único de tokens com comparação multiset — remoção OU invenção é Error. "Marcar revisada" exige validação sem Error (bypass "modo avançado" da spec fica pra reinserção, Sprint 5). Edição manual grava na TM (melhor fonte de tradução) e revalida na hora.

**Consequências:** filtro "placeholder mismatch" cobre as três famílias; falso positivo raro se destrava corrigindo o texto ou (futuro) modo avançado.

**Driver:** claude (ponytail).

## [2026-09-14] Reinserção: apply all-or-nothing e verify no arquivo relido

**Contexto:** spec §15 exige trabalho em working copy com validação posterior; aplicação parcial deixaria imagem meio-traduzida sem aviso.

**Decisão:** `apply_text` é all-or-nothing (qualquer tradução que não serializa → Err, nada gravado). A verificação roda sobre os bytes RELIDOS do disco (não os da memória) — o que foi persistido é o que vale. Entries com status Error bloqueiam a reinserção; `allow_errors` é o "modo avançado" da spec §14 (no core e no command; UI ainda passa false).

**Consequências:** working copy ou está íntegra e verificada, ou não existe (falha de verify mantém o arquivo só para inspeção, com erro claro).

**Driver:** claude.
