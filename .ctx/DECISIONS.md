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

## [2026-09-15] API key no keychain nativo; secrets.json vira fallback

**Contexto:** decisao de 14/09 previa keyring "quando o app for distribuido". Implementado com keyring 4 (apple-native + stores default de Windows/Linux).

**Decisao:** save grava no keychain e REMOVE qualquer secrets.json (nunca deixar copia plaintext quando o cofre aceitou); load consulta keychain primeiro e migra chave legada do arquivo automaticamente na primeira leitura; se o keychain nao estiver disponivel (ex.: Linux sem secret service), cai no arquivo 0600 com warn — degrada, nao bloqueia.

**Aprendizado de processo (registrado junto):** o commit da TM global (8258114) nao compilava o crate desktop — a validacao rodou num pipeline em background onde o clippy falhou mas linhas subsequentes commitaram mesmo assim, e o CI so valida o core. Correcoes: validacao SEMPRE em foreground antes de commit; P2 do CI de build do desktop subiu de prioridade.

**Driver:** rhuan (pediu o keychain) + claude.

## [2026-09-15] PlayStation: ISO 9660 unico p/ PS1/PS2/PSP; raw 2352 extrai mas nao reinsere

**Contexto:** as tres plataformas usam ISO 9660 (layout confirmado no iso_fs.h do kernel Linux). PS1 circula como BIN raw 2352 (setores com sync/header/EDC/ECC); PS2/PSP como ISO 2048. SYSTEM.CNF discrimina PS1 (BOOT=) de PS2 (BOOT2=) — metodo canonico; PSP identifica por UMD_DATA.BIN + PSP_GAME/PARAM.SFO (SFO parseado, psdevwiki).

**Decisao:** um parser compartilhado (`iso9660.rs`) com SectorMap 2048/raw; extracao por REGIAO de setor no raw (offsets absolutos corretos; strings que cruzam fronteira de setor sao perdidas — documentado); reinsercao in-place so em 2048 — em raw, cada escrita invalidaria EDC/ECC do setor, entao recusa com orientacao (converter para ISO). EDC/ECC (ECMA-130) e o proximo P1 para reinsercao raw direta. Limite em RAM (2 GiB) cobre PS1/PSP/PS2-CD; DVD dual-layer real pede streaming (P2).

**Driver:** rhuan (pediu PS1/PS2/PSP) + claude (verificacao kernel/psdevwiki antes de codar).

## 2026-09-15 — EDC/ECC transcrito do ECM com golden values da referência compilada

**Contexto:** reinserir em BIN raw 2352 exige regenerar EDC (CRC-32 poly
0xD8018001) e ECC (RS P/Q sobre GF(2^8) poly 0x11D) de cada setor alterado.
Algoritmo canônico da cena: ecm.c do Neill Corlett (domínio público, mesma
base de cdrdao/mkpsxiso).

**Decisão:** transcrever pra `adapters/cdrom.rs` e travar a transcrição
compilando o ecm.c de referência NESTA máquina pra gerar golden values
(EDC + bytes de paridade de setor determinístico), embutidos em teste.
Regeneração só nos setores que o diff vs original mostrar alterados;
Mode 2 usa header ZERADO no ECC (só Mode 1 inclui o real); Form 2 só
recalcula EDC se o campo já era usado (é opcional).

**Pegadinha registrada:** no C original `data` e `ecc` são ponteiros
sobrepostos no mesmo setor — o passo Q lê a paridade P recém-escrita
(índices até 0x8C8). Em Rust são dois passes com slices distintos; um
`split_at_mut` ingênuo estoura índice no Q.

**Consequência:** PS1 raw tem ciclo completo (9/9 plataformas); verify de
PS1/PS2/PSP confere EDC da imagem raw inteira; fixtures raw do synth agora
nascem com EDC/ECC válidos (subheader XA form 1). Multi-track (.cue com
áudio) segue fora — usuário aponta o track de dados.

## 2026-09-15 — Streaming pra DVD de PS2: mmap na leitura, escrita pontual na reinserção

**Contexto:** DVD de PS2 tem 4.7–8.5 GiB; o teto em memória (IN_MEMORY_MAX,
2 GiB) barrava extração e reinserção. Mac de trabalho tem 8 GB de RAM.

**Decisão:** leitura por memory-map (`fileio::read_view`, memmap2) — todas
as APIs continuam recebendo `&[u8]`, o SO pagina sob demanda e nada muda
nos adapters. Reinserção acima do teto vira streaming: `plan_in_place`
(validação anti-drift + bytes prontos, fatorado do apply) + cópia do
arquivo + seek/write só nos trechos alterados. Guard duro: streaming só
pra ISO 9660 2048/setor, formato sem checksum global nem EDC/ECC — raw
2352 é CD (<1 GiB) e continua no caminho em memória. Round-trip do BPS
gigante: `verify_bps_against` compara o apply contra a working copy sem
alocar o target (TargetCopy lê do expected, prefixo já provado idêntico;
mais estrito que o apply, e nossos patches nem emitem TargetCopy).

**Como testa sem DVD real:** `reinsert_project_with_limit` injeta o teto —
teste diferencial força streaming num ISO sintético pequeno e exige
resultado byte a byte idêntico ao caminho em memória.

**Consequência:** extração/reinserção/patch de DVD dual layer inteiro com
RAM limitada ao page cache. Premissa documentada do mmap: ninguém altera
o arquivo durante a operação (a mesma da leitura normal).

## 2026-09-20 — Release: rascunho, binário não assinado e ubuntu-22.04

**Contexto:** o projeto estava sem binário — pra testar era preciso Rust +
pnpm + compilar, o que elimina quase todo o público de rom hacking.

**Decisões:**

1. **Release sai como RASCUNHO** (`releaseDraft: true`). Com matriz de 4
   plataformas e `fail-fast: false`, se o Windows quebrar o macOS ainda
   sobe — e um release público com 3 de 4 binários é pior que nenhum.
   Rascunho = o Rhuan confere os 4 anexos e clica em Publish.

2. **`workflow_dispatch` é dry run.** A tauri-action só cria release se
   receber `tagName`; passando vazio ela apenas compila. Com
   `uploadWorkflowArtifacts` os bundles ficam baixáveis na página do run.
   Dá pra testar o pipeline inteiro sem criar tag nem release.

3. **Guard de versão como primeiro step.** Tag `v0.2.0` com
   `tauri.conf.json` em 0.1.0 gera release rotulado errado, e o conserto é
   apagar tag + release. Falha em 1s, em vez de descobrir depois de 4
   builds. Ficou como step (não job separado) pra evitar a ginástica de
   `needs` + `if: always()` — roda 4x e custa nada.

4. **`ubuntu-22.04`, não `ubuntu-latest`.** glibc mais velha: o AppImage e
   o .deb rodam também em distro antiga. Conferido no actions/runner-images
   que 22.04 segue ativo e sem marca de deprecação (o macOS 14 é que está).

5. **Binários não assinados, e isso é documentado em voz alta.** Certificado
   Apple/Microsoft é pago e o projeto é custo zero. O efeito prático é o
   macOS dizer "o app está danificado" — quem não souber do `xattr -cr`
   conclui que o programa é quebrado. Por isso o aviso vai no README E no
   corpo do release, com o comando exato, em PT-BR e um resumo em inglês.

6. **Dois builds de macOS em vez de universal.** É o padrão do exemplo
   oficial da tauri-action; downloads menores e fica explícito pro usuário
   qual é o dele.

**Verificação:** build release local no M1 antes de shipar o workflow —
DMG de 5 MB, `.app` com identifier e versão corretos, assinatura
`adhoc/linker-signed` (confirma o aviso de não assinado).

## 2026-09-21 — Relocação de ponteiros: reconhecer TABELAS, nunca busca global

**Contexto:** tradução PT-BR é ~20–30% mais longa que o inglês e, sem
relocação, tinha que caber em bytes no espaço da original — o maior limite de
uso real do app. A spec (Camada C) manda: "Crie abstrações para tabelas de
ponteiros e relocação. **Nunca faça busca/substituição global de bytes como
estratégia padrão.**"

**Por que a spec está certa (conta feita antes de codar):** no GBA um
ponteiro de ROM é `0x08000000 + offset` em u32 LE. A tentação é procurar
esse valor no ROM inteiro e trocar toda ocorrência. Só que a instrução Thumb
LSR tem os bytes `0x08xx` — uma palavra de código pode ter exatamente o
valor de um ponteiro por coincidência, e trocá-la corrompe o jogo sem aviso.

**Decisão:**

1. **Ponteiro só conta dentro de uma TABELA:** 2+ palavras alinhadas
   consecutivas que apontam, CADA UMA, pro início exato de uma string
   terminada que o scanner extraiu. Coincidência dupla é desprezível;
   estrutura real (menu, `struct {nome, desc}`, literal pool com 2+ strings)
   é pega. Ponteiro isolado é ignorado de propósito — a string fica in-place.
2. **Anexar no fim, não reusar o padding 0xFF.** Zero heurística de "onde
   acaba o dado", zero risco de pisar em dado real. Custo: o ROM cresce, e um
   ROM de 16 MiB que cresce sai em BPS (IPS não endereça além de 16 MiB) — o
   export já escolhe BPS sozinho. `ponytail`: reusar o padding mantém o
   tamanho/IPS, mas exige a heurística; só se alguém precisar.
3. **Alinhar em 4** a string realocada: há engine que copia texto com
   LDM/STM, e no ARM7 leitura desalinhada rotaciona a palavra.
4. **Original fica intacto:** referência que a detecção não viu mostra o
   texto antigo em vez de lixo — degradação graciosa.
5. **Só relocar o que ESTOURA.** O que cabe continua in-place e os
   ponteiros não mudam — minimiza exposição.
6. **`max_bytes = None` pra realocáveis:** o `max_bytes` vai pro prompt da
   IA; sem ele a IA traduz natural em vez de espremer o texto.
7. **Validador avisa (não bloqueia) estouro de realocável:** relocação
   resolve espaço no ROM, não na tela — caixa de texto ou buffer de RAM do
   jogo podem não comportar. O aviso aponta o que conferir no emulador.
8. **Motor genérico em `adapters/pointers.rs`** (u32 LE com `base`
   parametrizado), GBA é o único usuário hoje. NDS ARM9 e PS1 EXE cabem no
   mesmo motor mudando a base; NES/SNES (16 bits por banco) NÃO — ambíguo
   demais pra detecção, pediria tabela declarada pelo usuário (estilo Atlas).

**Salvaguardas:** anti-drift (ponteiro tem que ainda valer base+offset
original), checagem de que nenhuma escrita in-place pisou num ponteiro de
tabela, auto-checagem final (todo ponteiro reapontado resolve pros bytes
novos), teto de 32 MiB (janela de ROM do GBA). All-or-nothing como o resto.

**Limites honestos:** ponteiro isolado (literal pool com 1 string) não
reloca; ponteiros pros espelhos de wait-state 0x0A/0x0C000000 não são
reconhecidos; só ASCII terminada (é o que o adapter GBA extrai).

## 2026-09-21 — Relocação no NDS e PS1: dentro dos ARQUIVOS, nunca no ARM9/EXE

**Corrige a decisão anterior** (item 8 da relocação do GBA), que dizia que
"NDS ARM9 e PS1 EXE cabem no mesmo motor mudando a base". Errado. A detecção
de tabela funcionaria; o que não funciona é ONDE gravar o texto novo.

**Por que o GBA é especial:** o cartucho é mapeado na memória e lido direto
pela CPU — o que se anexa ao arquivo fica acessível em `0x08000000+offset`.
No NDS e no PS1 o código é COPIADO pra RAM por um carregador com tamanho
declarado (ARM9: até 0x3BFE00, praticamente a RAM principal inteira; EXE
PS1: `t_size` no header). Logo depois da imagem na RAM vêm a BSS (zerada no
boot pelo crt0) e o heap. Texto anexado ali seria apagado ou sobrescrito.
No PS1 ainda há outro problema: o MIPS monta endereço com `lui` + `addiu`
em duas instruções, invisíveis pra busca de tabela.

**Decisão — relocar dentro dos arquivos de dados**, onde está a maior parte
do texto e o ponteiro é offset RELATIVO ao arquivo:

1. **NDS: o arquivo cresce.** Cópia nova no fim do ROM (alinhada 0x200,
   padding 0xFF como o do próprio ROM), FAT reapontada (toda entrada-alias),
   header 0x080 e 0x014 atualizados, CRC do header recalculado. É o que as
   ferramentas de rebuild de DS fazem: o jogo acha arquivo pela FAT. Campos
   conferidos no GBATEK antes de codar. Risco residual: jogo que aloca buffer
   de tamanho FIXO pro arquivo não vê o texto novo.
2. **PS1: sobra do último setor do arquivo.** O hardware lê setores inteiros
   (CdRead), então a sobra sempre chega à RAM, dentro do buffer que o jogo já
   aloca arredondado. Recusa se a sobra não estiver zerada (pode ser dado
   escondido). Tamanho atualizado no directory record nos dois endians.
   Capacidade pequena (< 2 KB por arquivo, dividida entre as realocadas) —
   vira `max_bytes` pra IA e pro validador. Risco residual: jogo que copia só
   `tamanho` bytes de um tamanho hardcoded.
3. **PS2/PSP: só in-place.** Leem arquivo byte a byte; a sobra do setor não
   chega garantida à memória.
4. **Run mínimo 3 pra offset relativo** (2 no GBA). Offset dentro de arquivo
   é número pequeno, comum em dado binário (tamanho, contagem, coordenada).
   Conta: metade das palavras pequenas e 1 início de string por KB → run
   falso de 2 sai ~1 a cada 15 arquivos de 1 MB; de 3, ~1 a cada 30 mil.
   Custo aceito: tabela de 2 entradas (menu sim/não) em arquivo não é
   detectada — a string fica in-place.

**Não coberto (e documentado):** offsets u16 (comuns em arquivo pequeno),
offset relativo a seção em vez de ao arquivo (ex.: BMG), ponteiro absoluto
em arquivo carregado em endereço fixo. Todos falham do jeito seguro: sem
detecção, a string fica in-place.

## 2026-09-21 — Uma leitura por trecho de bytes (bug antigo do NDS)

**Contexto:** o NDS varre cada arquivo em ASCII E em UTF-16LE. Texto ASCII
lido como UTF-16 vira "CJK" falso (`䕎⁗䅇䕍`) no MESMO offset, e o id da
entry é só o offset (`scan-<offset>`). No banco as duas leituras se fundiam
pelo id: a tradução da string ASCII ficava com o encoding UTF-16 da falsa, e
a reinserção gravava `N\0O\0V\0O\0...` por cima do ASCII — o jogo mostraria
só "N". A UI ainda exibia o texto falso no lugar do real. Existia desde o
Sprint 8; o teste de fluxo da relocação é que pegou.

**Decisão:** resolver na extração (`resolve_encoding_overlaps`): quando as
leituras se sobrepõem, fica UMA. É ASCII se as strings ASCII cobrem ≥ 3/4 do
run UTF-16 e ele não tem assinatura de kana (byte alto 0x30 em metade das
unidades); senão, é UTF-16. Não há regra universal: kana UTF-16 lido como
ASCII também vira lixo (`B0D0F0`), e a regra do kana cobre isso. Limite
conhecido (`ponytail` no código): texto japonês UTF-16 só de kanji com bytes
imprimíveis pode ser lido como ASCII — resolver por idioma de origem se
aparecer em jogo real.

**Salvaguarda geral:** `plan_in_place` recusa duas traduções gravando nos
mesmos bytes (vale pra todo adapter e protege projeto antigo com entries já
fundidas no banco). Projeto de NDS criado antes disto: re-extrair.
