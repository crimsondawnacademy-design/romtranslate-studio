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

## [2026-09-14] Projeto pessoal: separado da infra de trabalho

**Contexto:** o RomTranslate Studio é projeto PESSOAL do mantenedor; a infra de trabalho dele (servidores e agentes internos) pertence a outra operação.

**Decisão:** nenhum deploy, automação, cron ou registro deste projeto vai para infra de trabalho. O projeto vive nas máquinas locais + repositório GitHub próprio. Automação agendada, se um dia existir, fica em infra pessoal.

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

## [2026-09-15] Patch: IPS proprio em Rust; BPS adiado

**Contexto:** spec §16 prioriza IPS/BPS/xdelta. IPS e trivial (formato de 1983) e cobre cartuchos ate 16 MiB; BPS exigiria implementar delta encoding + CRC ou depender de binario externo.

**Decisao:** IPS em Rust puro no `patch.rs`: create emite records raw (com merge de gaps <6 bytes e desvio do offset 0x454F46); apply le records, RLE e truncate extension. Todo patch passa por round-trip interno antes do export. BPS entra como variant quando arquivo >16 MiB ou truncamento forem necessarios (plataformas de disco).

**Consequencias:** zero dependencia externa; patch compativel com Lunar IPS/Floating IPS/RetroArch. Limite de 16 MiB explicito no erro.

**Driver:** claude (ponytail).

## [2026-09-15] GBA: reinsercao conservadora in-place (Experimental)

**Contexto:** reinsercao estruturada de verdade exige conhecer tabelas/ponteiros de cada jogo. O caminho util mais curto para "roda no emulador" e traduzir cada string ASCII no espaco que ela ja ocupa.

**Decisao:** `extract_structured` do GBA = scan ASCII com `max_bytes` = tamanho do run (ids identicos aos do scanner generico — rodar os dois nao duplica entries). `apply_text` escreve in-place com sanity check (bytes atuais == original_bytes da entry, senao erro anti-drift), padding 0x00/0x20 conforme o run era null-terminated, e SEMPRE recalcula o header checksum (traduzir o titulo em 0xA0 e legitimo). Sem relocacao: traducao maior que o espaco e erro orientando encurtar — o validador ja avisa antes ("muito longa").

**Consequencias:** menus/textos curtos de muitos jogos GBA traduziveis hoje, marcado Experimental (sem promessa universal, spec §26). Textos com ponteiros/compressao ficam para adapters por-jogo/engine.

**Driver:** rhuan (pediu o caminho pro emulador) + claude.

## [2026-09-15] Sprint 8: NDS por CRC documentado; Wii sem extracao; Wii U adiado

**Contexto:** spec §5 manda comecar plataformas de disco/container por inspecao+filesystem. NDS e o alvo de maior valor (cartucho plaintext, UTF-16 comum). Wii cifra particoes (extracao exigiria keys — proibido pela spec §23). Formato Wii U (WUD/WUX/RPX) nao foi verificado com fonte confiavel.

**Decisao:** NDS ganhou o ciclo completo conservador (probe por CRC-16 do header + CAMPO do logo CRC 0xCF56 — o bitmap do logo nao entra no repo; FNT/FAT com bounds e anti-ciclo; scan por arquivo; in-place). GC/Wii: probe-only com evidencias, Wii avisa na propria evidencia que particoes cifradas nao serao extraidas. Wii U ficou FORA ate verificar o formato — chute de magic numbers e pior que ausencia. Limites divididos: inspecao (probe+hash streaming) ate 16 GiB; extract/reinsert (arquivo em RAM) ate 512 MiB.

**Consequencias:** NDS traduzivel hoje no fluxo inteiro (fixture; ROM real >16 MiB esbarra no IPS — BPS e o proximo); helper `inplace.rs` unifica GBA/NDS e ganha UTF-16 de graca.

**Driver:** claude (ponytail + spec §5/§23).

## [2026-09-15] BPS: apply completo, create linear; formato auto no export

**Contexto:** IPS nao representa truncamento nem offsets >16 MiB (NDS real). O beat/BPS resolve os dois e valida CRC-32 do source (patch nao aplica em arquivo errado).

**Decisao:** `apply_bps` implementa o formato INTEIRO (4 commands, varint canonico com +1, metadata, CRC triplo) — patches de terceiros aplicam. `create_bps` emite so SourceRead/TargetRead (modo linear): para diffs localizados de traducao e praticamente otimo; delta com suffix array (SourceCopy/TargetCopy no create) so se alguem precisar de patch menor. Export: auto escolhe IPS quando cabe (compatibilidade com ferramentas antigas) e BPS caso contrario; usuario pode forcar na UI.

**Consequencias:** NDS real e truncamentos exportaveis; zero deps novas; patch BPS recusa arquivo errado com mensagem clara (CRCs nomeados).

**Driver:** rhuan (pediu o BPS) + claude (ponytail no create linear).

## [2026-09-15] SNES: verify pela soma REAL; fixture com checksum de fabrica [evolui a decisao de 14/09]

**Contexto:** em 14/09 o probe/fixture validavam so o PAR complement^checksum (barato p/ deteccao). Com reinsercao SNES, o apply recalcula o par com a soma canonica (`snes_sum`: potencia de 2 direta; resto espelhado ate a parte baixa; layout exotico cai em soma simples — consistente porque o verify usa o MESMO algoritmo).

**Decisao:** `verify` do SNES agora confere a SOMA REAL (mais forte que o probe, que segue barato só no par). `make_snes_lorom` passou a gravar o checksum verdadeiro. O probe nao mudou.

**Consequencias:** working copy de SNES sai com checksum que emulador/hardware aceitam; NES nao tem checksum de header (iNES) — finalize e noop.

**Driver:** rhuan (pediu NES/SNES) + claude.

## [2026-09-15] Wii U: WUX e RPX verificados; WUD bruto fora; e_type nao e evidencia

**Contexto:** o probe estava adiado ate verificar formato em fonte confiavel. Verificado: WUX no wud.h do WudCompress (cemu-project) — "WUX0" + 0x1099D02E + sectorSize u32 + uncompressedSize u64; RPX/RPL no cafe_loader_rpl.h do decaf-emu — EABI_CAFE=0xCA, EABI_VERSION_CAFE=0xFE, EM_PPC=20, SHF_DEFLATED=0x08000000.

**Decisao:** probe detecta WUX (magic dupla + sanidade do sectorSize) e RPX/RPL (ELF32 BE PPC + OSABI/versao CA FE — ELF comum rejeitado). e_type ficou FORA das evidencias: fontes divergem (0xFE01 no wiiubrew vs 0xFF01 no elf2rpl do wut). WUD bruto fora: sem magic documentado confiavel no offset 0, e na pratica dumps circulam como WUX. Detect-only: disco cifrado (keys fora do projeto) e secoes RPX deflated.

**Driver:** rhuan (pediu Wii U) + claude (verificacao antes de codar).

## [2026-09-15] GameCube: ciclo completo via FST; limite em RAM sobe a 2 GiB

**Contexto:** disco GC e plaintext (sem cifra, sem checksum de disco sobre dados) — o unico disco onde o padrao in-place funciona hoje. Formato confirmado no Dolphin: fst_offset/fst_size u32 BE em 0x424/0x428; FST = entries de 12 bytes (name_offset com flag de dir no byte alto, offset, size/next) + string table; GC usa offset_shift 0.

**Decisao:** GameCube ganhou list_resources (walk com stack de ranges, bounds e caps), extracao por arquivo com resource_path e reinsercao in-place (finalize noop). IN_MEMORY_MAX subiu de 512 MiB para 2 GiB para a ISO real (1.46 GiB) passar — pico de RAM ~2x o arquivo no apply (clone); streaming por recurso se doer na pratica.

**Consequencias:** GC e a sexta plataforma com ciclo completo; patch de ISO real sai como BPS (auto). Wii segue detect-only (cifrado).

**Driver:** rhuan (pediu o FST) + claude.

## [2026-09-15] TM global: cascata com prioridade do projeto; sempre ligada

**Contexto:** a decisao de 14/09 previa a TM global como segundo banco consultado em cascata. Implementada como `global_tm.sqlite` no config dir do app.

**Decisao:** lookup consulta o PROJETO primeiro (traducao revisada daquele jogo vence) e a global como fallback; TODA traducao nova (provider ou edicao manual) grava nas duas. Sem toggle de configuracao — sempre ligada (config para valor que ninguem muda e ruido); falha ao abrir a global degrada com warn, nunca bloqueia. Glossario global fica de fora ate haver demanda.

**Driver:** rhuan (pediu a TM global) + claude (ponytail no sem-toggle).
