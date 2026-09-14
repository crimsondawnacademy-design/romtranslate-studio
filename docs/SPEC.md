# RomTranslate Studio — Master Build Specification for Claude Code

## 1. Papel do agente

Você é o engenheiro principal responsável por transformar este repositório em um aplicativo desktop open source, local-first, modular e extensível para tradução assistida por IA de jogos retro e dumps legalmente obtidos pelo próprio usuário.

O projeto deve ser desenvolvido de forma incremental, executável e testável a cada etapa. Não implemente tudo de uma vez sem validar. Ao concluir cada fase, rode os testes, faça build e corrija os erros antes de continuar.

Nome do projeto: **RomTranslate Studio**.

Objetivo de produto: permitir que o usuário selecione localmente um arquivo de jogo compatível, identifique o formato/plataforma, extraia recursos de texto quando houver suporte técnico conhecido, traduza esses recursos usando um provedor configurável de IA ou tradução local, permita revisão, reintegre os textos quando possível e gere preferencialmente um patch/diff distributível, preservando a ROM/imagem original.

O aplicativo não deve incluir ROMs, BIOS, keys, firmware proprietário, material protegido por terceiros ou mecanismos para baixá-los.

---

## 2. Princípios obrigatórios

1. **Local-first**: arquivos de jogo permanecem no computador do usuário, exceto os trechos de texto explicitamente enviados ao provedor de tradução escolhido.
2. **Open source**: arquitetura adequada para GitHub, contribuições e plugins comunitários.
3. **Patch-first**: sempre que tecnicamente possível, o resultado padrão deve ser um patch, não uma cópia redistribuível do jogo.
4. **Não destrutivo**: nunca sobrescrever o arquivo original sem opção explícita e confirmação clara do usuário.
5. **Arquitetura por plugins/adapters**: suporte a plataformas e formatos deve ficar isolado do core.
6. **Provedores de tradução intercambiáveis**: OpenAI-compatible, Ollama e demais integrações devem implementar a mesma interface.
7. **Reprodutibilidade**: operações devem produzir logs, manifestos e checksums.
8. **Segurança**: secrets nunca podem ser gravados em logs nem commitados no repositório.
9. **Sem promessa falsa de compatibilidade universal**: jogos usam engines, compressões, tabelas e formatos diferentes. A UI deve distinguir claramente suporte completo, experimental, extração parcial e formato não suportado.
10. **Sem heurística destrutiva silenciosa**: qualquer escrita binária deve ocorrer em cópia de trabalho e possuir validação posterior.

---

## 3. Stack recomendada

Use preferencialmente:

- **Desktop shell**: Tauri 2
- **Frontend**: React + TypeScript + Vite
- **Backend/core**: Rust
- **Package manager JS**: pnpm
- **Workspace**: Cargo workspace + pnpm workspace quando útil
- **Serialização**: serde
- **HTTP Rust**: reqwest
- **Async**: tokio
- **Banco local**: SQLite via rusqlite ou sqlx
- **Hash**: SHA-256
- **Logs**: tracing + tracing-subscriber
- **CLI**: clap
- **Testes**: cargo test + Vitest para frontend
- **Formatting/lint**: rustfmt, clippy, ESLint, Prettier
- **CI**: GitHub Actions

Não adicione dependências pesadas sem necessidade. Prefira crates maduros e bem mantidos.

---

## 4. Estrutura alvo do monorepo

Crie ou migre o repositório para algo próximo de:

```text
romtranslate-studio/
├── apps/
│   ├── desktop/                 # Tauri + React UI
│   └── cli/                     # CLI opcional, usa o mesmo core
├── crates/
│   ├── core/                    # Orquestração do pipeline
│   ├── formats/                 # Detecção e primitives binárias
│   ├── translation/             # Providers de tradução
│   ├── translation-memory/      # TM + glossário
│   ├── patching/                # Patch backends
│   ├── project/                 # Manifesto de projeto
│   └── plugin-api/              # Contratos para adapters
├── plugins/
│   ├── gba/
│   ├── nes/
│   ├── snes/
│   ├── nds/
│   ├── gamecube/
│   ├── wii/
│   └── wiiu/
├── fixtures/                    # Apenas dados sintéticos / homebrew livre
├── docs/
├── scripts/
├── .github/workflows/
├── LICENSE
├── CONTRIBUTING.md
├── SECURITY.md
├── README.md
└── CLAUDE.md
```

Se uma divisão mais simples for melhor no primeiro commit, comece menor, mas preserve interfaces que permitam chegar a essa organização.

---

## 5. Escopo realista por plataforma

Não trate consoles como se cada um tivesse um formato de texto único. O suporte deve ser descrito como **tooling + adapters + formatos conhecidos**.

### Fase inicial / MVP

Prioridade:

- GBA
- NES
- SNES

O primeiro MVP não precisa traduzir “qualquer jogo” dessas plataformas. Precisa demonstrar um pipeline completo com fixtures sintéticas e adapters seguros, além de mecanismos de descoberta de strings.

### Fases seguintes

- Nintendo DS
- GameCube
- Wii
- Wii U
- eventualmente PS1 e outras plataformas

Para sistemas em disco/container, implemente primeiro inspeção, extração de filesystem e hooks para codecs/arquivos conhecidos. Evite prometer reinserção genérica.

### Dolphin

“Dolphin” é emulador, não plataforma. O escopo relacionado a Dolphin é GameCube/Wii. O aplicativo pode gerar arquivos/patches compatíveis com workflows do Dolphin quando isso fizer sentido, sem depender obrigatoriamente do Dolphin.

---

## 6. Pipeline central

Modele o processo como etapas explícitas:

```text
Input game file
  -> fingerprint/hash
  -> platform/container detection
  -> project creation
  -> resource discovery
  -> text extraction
  -> normalization
  -> segmentation
  -> translation memory lookup
  -> glossary application
  -> machine translation
  -> validation
  -> human review/editing
  -> encoding/layout checks
  -> reinsertion into working copy
  -> structural verification
  -> patch generation
  -> export report
```

Cada etapa deve possuir estado, logs, erros estruturados e possibilidade de retry quando adequado.

---

## 7. Modelo de domínio mínimo

Implemente tipos equivalentes a:

```rust
pub struct GameProject {
    pub id: Uuid,
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub platform: Platform,
    pub adapter_id: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub status: ProjectStatus,
    pub created_at: DateTime<Utc>,
}

pub struct TextEntry {
    pub id: String,
    pub resource_path: Option<String>,
    pub offset: Option<u64>,
    pub original_bytes: Vec<u8>,
    pub source_text: String,
    pub translated_text: Option<String>,
    pub context: Option<String>,
    pub max_bytes: Option<usize>,
    pub encoding: TextEncoding,
    pub status: TranslationStatus,
    pub metadata: serde_json::Value,
}
```

Inclua enums claros para `Platform`, `SupportLevel`, `ProjectStatus`, `TranslationStatus` e `TextEncoding`.

---

## 8. API de plugin / adapter

Crie uma interface estável. Um adapter deve declarar capacidades e limitações.

Exemplo conceitual:

```rust
#[async_trait]
pub trait GameAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> AdapterCapabilities;

    async fn probe(&self, input: &GameInput) -> Result<ProbeResult>;
    async fn discover(&self, ctx: &ProjectContext) -> Result<Vec<ResourceDescriptor>>;
    async fn extract_text(&self, ctx: &ProjectContext) -> Result<Vec<TextEntry>>;
    async fn apply_text(&self, ctx: &ProjectContext, entries: &[TextEntry]) -> Result<ApplyReport>;
    async fn verify(&self, ctx: &ProjectContext) -> Result<VerificationReport>;
}
```

`AdapterCapabilities` deve informar, por exemplo:

- detect
- extract
- reinsert
- patch
- compression support
- pointer relocation support
- font/table support
- experimental flag

Não carregue plugins nativos arbitrários da internet no MVP. Comece com adapters compilados no workspace. Deixe a ABI/plugin loading dinâmica para uma fase posterior.

---

## 9. Detecção do arquivo

Implemente detecção segura por:

- extensão como pista, nunca como única fonte;
- magic bytes/header;
- tamanho e estrutura;
- checksums opcionais;
- probe dos adapters.

Resultado deve exibir confiança e motivo:

```text
Platform: Game Boy Advance
Confidence: 0.98
Evidence:
- Nintendo logo/header structure valid
- cartridge header checksum valid
```

---

## 10. Extração de texto

Este é o componente mais difícil. Construa em camadas.

### Camada A — String scanner genérico

Suporte a scanners configuráveis para:

- ASCII
- UTF-8
- UTF-16 LE/BE
- Shift-JIS quando apropriado
- tabelas customizadas `.tbl`

Parâmetros:

- comprimento mínimo
- bytes permitidos
- terminadores
- regiões de busca
- alinhamento

O scanner genérico serve para descoberta e debug, não garante reinserção segura.

### Camada B — Recursos estruturados

Adapters específicos devem reconhecer formatos conhecidos e preservar:

- offsets
- ponteiros
- terminadores
- códigos de controle
- placeholders
- tags
- compressão
- limite de bytes

### Camada C — Pointer tables

Crie abstrações para tabelas de ponteiros e relocação. Nunca faça busca/substituição global de bytes como estratégia padrão.

### Camada D — Compressão

Arquitetura deve permitir codecs plugáveis. Só implemente algoritmos quando houver testes e fixtures legais/sintéticas.

---

## 11. Tradução por IA

Crie trait única:

```rust
#[async_trait]
pub trait TranslationProvider {
    async fn translate_batch(&self, request: TranslationRequest) -> Result<TranslationResponse>;
    async fn health_check(&self) -> Result<()>;
}
```

Providers iniciais:

1. **Ollama** — gratuito/local
2. **OpenAI-compatible HTTP API** — permite OpenAI e endpoints compatíveis
3. **OpenRouter-compatible** opcional

Não hardcode modelos específicos. Use configuração.

A chave de API deve ficar em keychain/secret storage quando possível, nunca no repositório.

---

## 12. Prompt de tradução

A tradução deve usar saída estruturada e preservar tokens especiais.

Prompt base conceitual:

```text
You are translating text from a video game.
Target locale: pt-BR.
Preserve all placeholders, control codes, tags and escape sequences exactly.
Respect character names and glossary entries.
Do not add explanations.
Keep tone consistent with context.
If a text has a byte/length limit, prefer concise wording.
Return valid JSON matching the requested schema.
```

Envie batches limitados. Forneça contexto próximo apenas quando necessário.

Nunca envie bytes da ROM inteira a APIs de terceiros.

---

## 13. Translation Memory e glossário

Use SQLite local.

Tabelas mínimas:

- projects
- source_segments
- translations
- glossary_terms
- provider_runs
- exports

A Translation Memory deve usar uma chave derivada de:

```text
normalized source text + source locale + target locale + optional game scope
```

Glossário deve suportar:

- termo original
- tradução preferida
- “não traduzir”
- case sensitivity
- observação/contexto
- escopo global ou por projeto

Exemplo:

```text
Potion -> Poção
HP -> NÃO TRADUZIR
Link -> NÃO TRADUZIR
Save -> Salvar
```

---

## 14. Validação de tradução

Antes de aplicar:

- validar placeholders;
- comparar control codes;
- validar tags;
- verificar limite de bytes no encoding destino;
- detectar strings vazias indevidas;
- detectar mudança anormal de comprimento;
- marcar itens que precisam de revisão.

Exemplo:

```text
Original: "HP {0}: 120"
Translation: "PV: 120"
ERROR: placeholder {0} was removed
```

A UI deve bloquear reinserção de erros críticos, salvo modo avançado explicitamente habilitado.

---

## 15. Reinserção

Trabalhe sempre em uma cópia de trabalho.

Fluxo:

1. verificar SHA-256 da origem;
2. criar working copy;
3. serializar traduções no encoding correto;
4. atualizar offsets/ponteiros quando suportado;
5. recalcular checksums internos quando aplicável;
6. executar `verify()`;
7. somente então permitir export.

Se uma tradução ultrapassar um campo fixo e não houver relocação suportada, marque como erro e peça texto menor.

---

## 16. Patches

Arquitetura de patching deve ser extensível.

Prioridades:

- IPS quando adequado
- BPS quando houver backend confiável
- xdelta/VCDIFF para arquivos maiores, se disponível de forma multiplataforma

Para o MVP, é aceitável integrar um executável externo opcional somente se:

- licença permitir;
- origem estiver documentada;
- binário não for baixado silenciosamente;
- usuário puder configurar caminho;
- erro for tratado claramente.

Preferir bibliotecas Rust quando maduras.

Export deve produzir:

```text
MyGame.pt-BR.bps
MyGame.pt-BR.manifest.json
MyGame.pt-BR.translation.csv (opcional)
```

Manifesto:

```json
{
  "project": "RomTranslate Studio",
  "source_sha256": "...",
  "target_locale": "pt-BR",
  "adapter": "gba.generic-v1",
  "translated_entries": 1234,
  "reviewed_entries": 1180,
  "patch_format": "BPS"
}
```

---

## 17. UI/UX

Visual deve ser limpo e desktop-first.

### Tela inicial

- Novo projeto
- Abrir projeto
- Projetos recentes
- Configurações

### Wizard de novo projeto

Passo 1: selecionar arquivo

Passo 2: resultado da detecção

```text
Pokémon Emerald.gba
Platform: Game Boy Advance
Adapter: GBA Generic
Support: Experimental
```

Passo 3: idiomas

- idioma de origem: automático/manual
- idioma destino: default `pt-BR` se locale do sistema for português do Brasil

Passo 4: tradução

- Ollama
- OpenAI-compatible
- modo “somente extrair, não traduzir”

Passo 5: criar projeto

### Editor principal

Layout recomendado:

```text
┌ Sidebar ───────┬────────────────────────────────────────┐
│ Overview       │ Original                               │
│ Strings        │ "Welcome to the village!"             │
│ Glossary       │                                        │
│ Translation TM │ Translation                            │
│ Export         │ "Bem-vindo à vila!"                   │
│ Logs           │                                        │
│                │ Context / bytes / status               │
└────────────────┴────────────────────────────────────────┘
```

Tabela de strings:

- ID
- original
- tradução
- status
- bytes original
- bytes destino
- contexto
- flags

Filtros:

- untranslated
- machine translated
- reviewed
- error
- too long
- placeholder mismatch

### Progresso

Mostrar etapas reais:

```text
[✓] Arquivo identificado
[✓] 1.842 strings encontradas
[✓] 1.620 recuperadas da memória
[■] Traduzindo 222 restantes... 61%
[ ] Validação
[ ] Geração do patch
```

---

## 18. Configurações

Configuração local:

```toml
[translation]
default_provider = "ollama"
default_target_locale = "pt-BR"

[ollama]
base_url = "http://localhost:11434"
model = ""

[openai_compatible]
base_url = "https://api.openai.com/v1"
model = ""

[privacy]
allow_remote_translation = false
```

Secrets separados do arquivo normal de config.

---

## 19. CLI

O CLI deve reutilizar o core.

Exemplos desejados:

```bash
romtranslate inspect game.gba
romtranslate extract game.gba --out strings.json
romtranslate translate project.rts --provider ollama --target pt-BR
romtranslate validate project.rts
romtranslate export project.rts --patch bps
```

Não é obrigatório terminar o CLI no primeiro sprint, mas a arquitetura não pode prender o core à UI.

---

## 20. Formato de projeto

Crie extensão lógica `.rtsproj` ou diretório de projeto.

Sugestão:

```text
MyGame.rtsproj/
├── project.json
├── translations.sqlite
├── cache/
├── extracted/
├── working/
└── exports/
```

Não copie o arquivo original para dentro do projeto por padrão. Guarde path + hash e avise se ele mudar.

---

## 21. Testes

Não use ROMs comerciais em fixtures.

Crie fixtures sintéticas que simulem:

- header válido
- string table
- pointer table
- strings de tamanho fixo
- strings relocáveis
- placeholders
- control codes

Testes obrigatórios:

- detecção
- scanning
- encoding
- pointer relocation
- placeholder validator
- translation memory
- export/import do projeto
- patch round-trip quando possível
- nenhuma escrita no arquivo original

Use property tests quando fizer sentido para parsers binários.

---

## 22. Segurança e robustez

- limite de tamanho de arquivo configurável;
- cuidado com integer overflow em offsets;
- path traversal em containers;
- decompression bombs;
- input malformado;
- timeouts de provider;
- retry com backoff;
- cancelamento de operação;
- atomic writes;
- backup da working copy;
- logs sem secrets.

Toda leitura binária deve validar limites antes de acessar slices.

---

## 23. Legal / posicionamento do projeto

Inclua no README uma mensagem semelhante a:

> RomTranslate Studio é uma ferramenta de tradução e modificação para arquivos de jogos obtidos legalmente pelo próprio usuário. O projeto não distribui jogos, BIOS, keys, firmware ou conteúdo proprietário. Incentivamos a distribuição de patches em vez de imagens completas modificadas e o respeito às leis e licenças aplicáveis.

Não implemente busca ou download de ROMs.

Não inclua links para sites de ROMs.

---

## 24. GitHub

Criar:

### README.md

Inclua:

- screenshot placeholder
- descrição
- status alpha
- funcionalidades
- plataformas
- como instalar
- Ollama
- provider remoto
- como contribuir
- aviso legal

### CONTRIBUTING.md

Explique:

- ambiente
- branches
- testes
- padrão de commits
- criação de adapters

### SECURITY.md

Canal para bugs de parser, path traversal, exposição de chaves etc.

### Issues templates

- bug
- adapter request
- game compatibility report
- feature request

### GitHub Actions

Jobs iniciais:

```text
check
- cargo fmt --check
- cargo clippy --all-targets --all-features -- -D warnings
- cargo test --workspace

frontend
- pnpm install --frozen-lockfile
- pnpm lint
- pnpm test
- pnpm build
```

Depois adicionar builds Tauri para macOS/Windows/Linux.

---

## 25. Roadmap de implementação

### Sprint 0 — Bootstrap

Objetivo: repo saudável.

- criar workspaces
- Tauri abre
- React abre
- core Rust compilando
- lint/test CI
- README inicial

**Definition of Done:** `pnpm tauri dev` abre uma janela e `cargo test --workspace` passa.

### Sprint 1 — Project + file inspection

- file picker
- SHA-256
- detector interface
- NES/SNES/GBA basic probes
- tela de informações do arquivo
- criar projeto local

**DoD:** usuário seleciona uma fixture ou arquivo local e vê plataforma, hash, tamanho e adapter selecionado.

### Sprint 2 — Text extraction framework

- `TextEntry`
- ASCII/UTF scanners
- custom table loader
- generic extraction view
- export JSON/CSV

**DoD:** fixtures retornam strings com offsets e metadata reproduzíveis.

### Sprint 3 — Translation

- provider trait
- Ollama
- OpenAI-compatible
- batching
- retries
- progress
- glossary
- translation memory

**DoD:** strings sintéticas podem ser traduzidas, salvas e reabertas.

### Sprint 4 — Validation/editor

- editor de strings
- placeholder checks
- byte length checks
- statuses
- manual review
- search/filter

**DoD:** erros aparecem antes de qualquer reinserção.

### Sprint 5 — Reinsertion fixture adapter

Crie primeiro um adapter sintético demonstrando:

- fixed strings
- relocatable strings
- pointer table update
- checksum

Depois aplique padrões a adapters reais suportados.

**DoD:** teste round-trip prova que alteração e leitura posterior funcionam.

### Sprint 6 — Patch export

- patch abstraction
- um formato funcional
- manifest
- export UI

**DoD:** patch aplicado à fixture original produz exatamente o arquivo esperado.

### Sprint 7 — Plataforma/community

- adapter docs
- compatibility matrix
- plugin SDK inicial
- templates de contribuição

### Sprint 8+ — NDS/GameCube/Wii/Wii U

Adicionar suporte gradualmente, priorizando arquivos/engines específicos em vez de afirmar compatibilidade universal.

---

## 26. Compatibilidade exibida ao usuário

Crie níveis:

```rust
pub enum SupportLevel {
    Full,
    Partial,
    ExtractOnly,
    Experimental,
    Unsupported,
}
```

Exemplo na UI:

```text
Game Boy Advance
Generic text scanning: Supported
Structured extraction: Partial
Automatic reinsertion: Experimental
Patch export: Supported
```

Isso evita vender a ideia falsa de “qualquer ROM”.

---

## 27. Observabilidade

Logs precisam ter níveis:

- INFO: progresso útil
- WARN: comportamento experimental
- ERROR: falha concreta
- DEBUG: offsets, adapter decisions

Exemplo:

```text
INFO project created source_sha256=...
INFO adapter selected id=gba.generic confidence=0.94
INFO extracted entries=1842
WARN entries_with_unknown_control_codes=17
INFO translation_memory_hits=1620
```

Inclua botão “Export diagnostic report” que remova paths/secrets sensíveis antes de compartilhar.

---

## 28. Performance

- streaming/chunked IO para arquivos grandes;
- não ler discos inteiros repetidamente;
- cache de resultados pelo hash;
- translation batches concorrentes com limite configurável;
- UI nunca pode travar durante processamento;
- cancel token no pipeline.

---

## 29. Internacionalização do próprio app

Prepare i18n desde o começo.

Idiomas iniciais da UI:

- pt-BR
- en-US

Use IDs de mensagem, não strings espalhadas em componentes.

---

## 30. O que NÃO fazer

Não:

- implementar download de jogos;
- procurar ROMs na internet;
- incluir BIOS/keys;
- modificar o original por padrão;
- prometer suporte universal;
- mandar binário completo do jogo para IA;
- criar parsing com offsets hardcoded sem testes/documentação;
- esconder warnings de corrupção;
- salvar API key em plaintext dentro do projeto;
- acoplar UI diretamente aos parsers;
- usar uma única função gigante de “translate_rom”.

---

## 31. Primeira execução para Claude Code

Ao receber este documento, execute nesta ordem:

1. Inspecione o repositório atual.
2. Liste arquivos e identifique o que já existe.
3. Crie um plano curto para **Sprint 0 e Sprint 1 apenas**.
4. Não apague trabalho existente sem necessidade.
5. Inicialize/migre o workspace.
6. Implemente Tauri + React + Rust core mínimo.
7. Implemente seleção de arquivo e SHA-256.
8. Implemente traits de adapter/detector.
9. Implemente probes básicos para GBA, NES e SNES.
10. Crie fixtures sintéticas para testes.
11. Implemente tela que mostra resultado da detecção.
12. Adicione testes.
13. Rode formatter, lint, tests e build.
14. Corrija tudo até ficar verde.
15. Atualize README com instruções reais de execução.
16. Faça um resumo do que mudou, comandos para executar e próximo sprint recomendado.

Não pule diretamente para tradução/reinserção antes do pipeline básico estar saudável.

---

## 32. Critérios de qualidade do código

- funções pequenas e nomeadas claramente;
- erros com contexto usando `thiserror`/`anyhow` conforme camada;
- core não depende de Tauri;
- interfaces mockáveis;
- sem `unwrap()` em caminhos de input do usuário, salvo em testes;
- parser binário sempre checa bounds;
- comentários explicam “por quê”, não o óbvio;
- documentação pública para traits importantes;
- commits logicamente separáveis.

---

## 33. Meta de produto v0.1

A primeira versão publicável não precisa traduzir todo jogo.

Ela precisa provar:

1. instalação desktop simples;
2. seleção local de arquivo;
3. identificação de plataforma;
4. extração reproduzível de texto em formatos/adapters suportados;
5. tradução via Ollama ou API configurada pelo usuário;
6. memória + glossário;
7. revisão e validação;
8. reinserção em adapters explicitamente suportados;
9. geração de patch;
10. documentação que permita à comunidade adicionar adapters.

---

## 34. Visão futura

Depois do v0.1, considerar:

- SDK de adapters externos;
- marketplace/index comunitário apenas de plugins, nunca ROMs;
- perfis por engine/jogo;
- OCR de texturas/imagens para jogos que renderizam texto como imagem;
- font atlas editor;
- preview de caixas de diálogo;
- detecção de overflow visual;
- integração opcional com emuladores para preview rápido;
- collaborative translation via arquivos de projeto/Git;
- tradução incremental após updates de patches;
- QA linguístico com LLM;
- import/export PO, XLIFF e CSV.

---

## 35. Nome e posicionamento

Nome de trabalho: **RomTranslate Studio**.

Tagline:

> Local-first, AI-assisted game translation toolkit.

Descrição curta em português:

> Ferramenta open source para extrair, traduzir, revisar e aplicar textos em jogos compatíveis, com IA local ou APIs configuráveis e geração de patches.

---

# INSTRUÇÃO FINAL PARA O CLAUDE CODE

Comece agora pela inspeção do repositório e pela implementação dos **Sprints 0 e 1**. Trabalhe diretamente nos arquivos. Não responda apenas com pseudocódigo ou planejamento. Crie código funcional, testes e documentação. Ao final, execute todos os comandos de validação disponíveis e apresente:

1. arquivos criados/alterados;
2. arquitetura implementada;
3. comandos exatos para rodar o app;
4. resultado dos testes/build;
5. limitações atuais;
6. próxima tarefa recomendada (Sprint 2).
