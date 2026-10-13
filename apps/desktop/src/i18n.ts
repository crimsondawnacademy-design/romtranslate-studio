// i18n minimo por IDs de mensagem (spec §29). Sem lib: um dict por locale e
// interpolacao {param}. Quando crescer, migrar para lib mantendo os IDs.

export type Locale = "pt-BR" | "en-US";

const messages = {
  "pt-BR": {
    "app.title": "RomTranslate Studio",
    "app.tagline": "Tradução de jogos assistida por IA — local-first",
    "home.selectFile": "Selecionar arquivo de jogo",
    "home.openProject": "Abrir projeto",
    "home.hint":
      "Selecione ou arraste para esta janela uma ROM ou dump obtido legalmente por você (ou uma pasta de projeto .rtsproj). O arquivo nunca sai do seu computador nesta etapa.",
    "home.drop": "Solte para abrir.",
    "home.title": "Arraste o arquivo do jogo para cá",
    "inspect.loading": "Inspecionando arquivo...",
    "inspect.title": "Resultado da detecção",
    "inspect.platform": "Plataforma",
    "inspect.adapter": "Adapter",
    "inspect.confidence": "Confiança",
    "inspect.support": "Suporte",
    "inspect.evidence": "Evidências",
    "inspect.file": "Arquivo",
    "inspect.size": "Tamanho",
    "inspect.sha256": "SHA-256",
    "inspect.noMatch":
      "Formato não reconhecido pelos adapters atuais. Você ainda pode ver hash e tamanho acima.",
    "inspect.otherCandidates": "Outros candidatos",
    "support.full": "Completo",
    "support.partial": "Parcial",
    "support.extract_only": "Somente extração",
    "support.experimental": "Experimental",
    "support.unsupported": "Não suportado",
    "project.create": "Criar projeto",
    "project.otherGame":
      "Já existe um projeto em {dir}, mas de outro arquivo de jogo (o conteúdo é diferente). Abra-o por \"Abrir projeto\", ou mova ou renomeie essa pasta para criar um novo.",
    "project.creating": "Criando projeto...",
    "project.sourceLanguage": "Idioma de origem",
    "project.targetLanguage": "Idioma de destino",
    "project.auto": "Detectar depois",
    "project.createdTitle": "Projeto criado",
    "project.createdAt": "Projeto salvo em:",
    "project.extract": "Extrair strings",
    "project.sourceMissing":
      "A ROM de origem não está mais no caminho gravado no projeto. Coloque o arquivo de volta e reabra.",
    "project.sourceChanged":
      "Atenção: o arquivo de origem mudou desde a criação do projeto (SHA-256 diferente).",
    "extract.title": "Extração de strings",
    "extract.hint":
      "Para traduzir e regravar o jogo, use a Extração estruturada: ela guarda o espaço de cada frase e limpa o que não é texto. O Escanear é o scanner genérico, para descoberta.",
    "extract.encoding": "Codificação",
    "extract.minChars": "Mín. caracteres",
    "extract.chooseTbl": "Escolher .tbl",
    "extract.tblMissing": "Escolha um arquivo .tbl para escanear com tabela.",
    "extract.scan": "Escanear",
    "extract.scanning": "Escaneando...",
    "extract.found": "{n} strings encontradas",
    "extract.truncated": "a varredura parou no teto: há mais sequências no arquivo além destas.",
    "extract.onlyText": "Só o que parece texto",
    "extract.onlyTextHint":
      "Descarta tile, código e tabela que por acaso são bytes imprimíveis. Vale para o botão Escanear; desmarque para ver tudo.",
    "extract.discarded":
      "{n} sequências descartadas por não parecerem texto. Para ver tudo, desmarque “Só o que parece texto” e escaneie de novo. O que já estava no projeto continua na lista.",
    "extract.discardedStructured":
      "{n} sequências descartadas por não parecerem texto. A extração estruturada sempre filtra, porque o que ela devolve é regravado no jogo; string com ponteiro em tabela nunca é descartada. Para ver tudo, use Escanear com “Só o que parece texto” desmarcado.",
    "extract.removedOld":
      "{n} strings antigas sem tradução saíram do projeto por não parecerem texto. Ficou o que tem tradução, o que tem ponteiro em tabela e o que foi lido por tabela de caracteres.",
    "extract.none": "Nenhuma string com essa configuração.",
    "extract.emptyTitle": "Nenhum texto extraído ainda",
    "extract.emptyHint":
      "Clique em “Extração estruturada” para puxar os textos do jogo. Eles aparecem aqui, prontos para traduzir.",
    "extract.filter": "Filtrar texto...",
    "extract.showing": "Mostrando {shown} de {total}",
    "extract.offset": "Offset",
    "extract.bytes": "Bytes",
    "extract.text": "Texto",
    "export.json": "Exportar JSON",
    "export.csv": "Exportar CSV",
    "export.saved": "Salvo em: {path}",
    "entries.saved": "{n} strings salvas no projeto",
    "table.translation": "Tradução",
    "table.resource": "Arquivo",
    "status.untranslated": "pendente",
    "status.machine": "máquina",
    "status.reviewed": "revisada",
    "status.error": "erro",
    "status.internal": "nome interno",
    "translate.title": "Tradução",
    "translate.provider": "Provider",
    "translate.baseUrl": "Base URL",
    "translate.model": "Modelo",
    "translate.apiKey": "API key",
    "translate.apiKeySet": "(já salva — deixe vazio para manter)",
    "translate.allowRemote": "Permitir tradução remota (envia os TEXTOS ao endpoint configurado)",
    "translate.codexCommand": "Comando do Codex",
    "translate.codexModel": "Modelo (opcional)",
    "translate.codexModelDefault": "padrão do Codex",
    "translate.codexHint":
      "Usa a sua assinatura do ChatGPT pelo Codex CLI, sem chave de API. O app não vê sua senha nem o token. Os textos do jogo vão para a OpenAI, por isso é preciso permitir tradução remota. Cada lote é uma chamada do Codex: projeto grande gasta o limite da assinatura.",
    "translate.codexAccount": "Conta",
    "translate.codexAccountSystem": "A do Codex deste computador",
    "translate.codexAccountApp": "Uma conta só deste app",
    "translate.codexLogin": "Entrar com o ChatGPT",
    "translate.codexSwitch": "Trocar de conta",
    "translate.codexLogout": "Sair",
    "translate.codexLoginCancel": "Cancelar login",
    "translate.codexLoginWaiting":
      "Conclua o login na página que abriu no navegador: é lá que você escolhe a conta. O app espera até 5 minutos.",
    "translate.codexChecking": "Conferindo o login...",
    "translate.codexLogged": "Conta conectada ao ChatGPT.",
    "translate.codexNotLoggedApp":
      "Esta conta ainda não tem login. Clique em \"Entrar com o ChatGPT\" e escolha a conta no navegador.",
    "translate.codexNotLoggedSystem":
      "O Codex deste computador não está logado. Rode `codex login` no terminal, ou escolha \"Uma conta só deste app\".",
    "translate.saveSettings": "Salvar configuração",
    "translate.remoteAsk":
      "Para usar este provider, os textos do jogo são enviados para fora do seu computador (à OpenAI, no caso do ChatGPT). O arquivo do jogo não é enviado. Permitir?",
    "translate.remoteAllow": "Permitir e continuar",
    "translate.providerClaudeKey": "Claude (chave de API)",
    "translate.claudeKeyHint":
      "Traduz com o Claude pela API da Anthropic, com a sua chave de API (criada no console da Anthropic). A chave fica no cofre do sistema, nunca no projeto. Os textos do jogo vão para a Anthropic, e o uso é cobrado por token na sua conta.",
    "translate.priceInput": "US$ por milhão de tokens (entrada)",
    "translate.priceOutput": "US$ por milhão de tokens (saída)",
    "translate.priceHint":
      "Preço do seu modelo, para a conta de gasto. Deixe 0 para usar a tabela dos modelos do Claude que o app conhece.",
    "translate.priceUsed":
      "Conta feita com US$ {input} (entrada) e US$ {output} (saída) por milhão de tokens. Confira na tabela de preços do provedor: o app não a atualiza sozinho.",
    "translate.priceMissing":
      "Sem preço informado: o app mostra só os tokens. Preencha os dois campos para ver o valor.",
    "translate.estimate": "Calcular gasto",
    "translate.estimating": "Calculando...",
    "translate.estimateResult":
      "Traduzir as {entries} frases pendentes são {batches} chamadas: cerca de {input} tokens de entrada e {output} de saída.",
    "translate.estimateCost": "Custo estimado: US$ {cost}.",
    "translate.estimateCalibrated":
      "Entrada contada pelo próprio modelo. A saída é uma estimativa: modelo que raciocina antes de responder gasta mais saída do que isso. A memória de tradução pode reduzir o total.",
    "translate.estimateRough":
      "Estimativa por média de caracteres por token, com margem de erro grande. Modelo que raciocina antes de responder gasta mais saída. A memória de tradução pode reduzir o total.",
    "translate.spent": "Cobrado nesta rodada: {input} tokens de entrada e {output} de saída.",
    "translate.spentCost": "Cerca de US$ {cost}.",
    "translate.batchSize": "Frases por chamada",
    "translate.batchSizeHint":
      "Quantas frases vão em cada chamada à IA (1 a 50). Mais frases por chamada são menos chamadas, o que pesa menos no limite de uma assinatura; se as respostas começarem a falhar, diminua.",
    "translate.stopped":
      "A tradução parou porque o provider deixou de responder: {error} — o que já foi traduzido está salvo e o resto continua pendente.",
    "translate.test": "Testar conexão",
    "translate.testing": "Testando...",
    "translate.testOk": "Conexão OK — modelo: {model}",
    "translate.run": "Traduzir pendentes",
    "translate.running": "Traduzindo...",
    "translate.cancel": "Cancelar",
    "translate.phase.tm": "memória de tradução",
    "translate.phase.translate": "traduzindo",
    "translate.phase.shorten": "encurtando as que não couberam",
    "translate.summaryShortened": " · {shortened} encurtadas pra caber",
    "translate.summaryTooLong": " · {tooLong} ainda não cabem",
    "translate.summary":
      "{translated} traduzidas · {tmHits} da memória · {failed} falharam",
    "translate.summaryCancelled": " · cancelado",
    "translate.internal":
      "{n} strings ficam de fora da tradução: nomes internos do jogo (sala, script: só maiúsculas, dígitos e sublinhado, sem espaço) e trechos soltos fora das regiões de texto, que costumam ser dado de gráfico ou de som. Não vão para a IA e não são regravadas. Para vê-las, filtre por “nomes internos”; se alguma for texto do jogo, selecione-a e clique em “É texto do jogo”.",
    "translate.cancelling": "Cancelando…",
    "translate.saved": "Configuração salva.",
    "extract.stray":
      "{n} strings estão fora das regiões de texto do jogo e saíram da fila de tradução (a tradução automática que alguma tinha foi descartada, para não ser gravada por cima de dado do jogo). Elas aparecem no filtro “nomes internos”.",
    "extract.packed":
      "{n} frases estavam dentro de blocos comprimidos (LZ77). A tradução de cada uma precisa caber no espaço da original; na reinserção o bloco é recomprimido.",
    "translate.ascii":
      "Texto ASCII não tem letra acentuada: a IA é avisada disso e, se mesmo assim responder com acento ou com aspas e travessão tipográficos, o app grava a letra sem acento e o sinal simples, com um aviso na frase. O que não tem equivalente bloqueia a reinserção e a frase diz qual caractere é.",
    "glossary.title": "Glossário",
    "glossary.term": "Termo",
    "glossary.translation": "Tradução",
    "glossary.noTranslate": "Não traduzir",
    "glossary.add": "Adicionar",
    "glossary.remove": "Remover",
    "glossary.empty": "Nenhum termo ainda. Termos guiam a IA (ex.: Potion → Poção; HP → não traduzir).",
    "filter.all": "todas",
    "filter.untranslated": "pendentes",
    "filter.machine": "traduzidas (máquina)",
    "filter.reviewed": "revisadas",
    "filter.error": "com erro",
    "filter.internal": "nomes internos",
    "filter.too_long": "muito longas",
    "filter.placeholder_mismatch": "placeholder quebrado",
    "validate.run": "Validar",
    "validate.summary": "{errors} erros · {warnings} avisos em {checked} traduzidas",
    "editor.pick": "Clique numa linha da tabela para editar a tradução.",
    "editor.original": "Original",
    "editor.translation": "Tradução",
    "editor.bytes": "{n} bytes",
    "editor.bytesLimit": "limite {max}",
    "editor.bytesOriginal": "original {n}",
    "editor.relocatable": "realocável — pode passar do original",
    "editor.noEncoder": "sem contagem p/ este encoding",
    "editor.save": "Salvar tradução",
    "editor.markReviewed": "Marcar revisada",
    "editor.unmarkReviewed": "Desmarcar revisão",
    "editor.internal":
      "Nome interno do jogo: fica de fora da tradução e não é regravado. A marca volta se você extrair de novo antes de traduzir.",
    "editor.unmarkInternal": "É texto do jogo",
    "extract.structured": "Extração estruturada",
    "extract.xray": "Raio-X",
    "xray.title": "Raio-X do arquivo",
    "xray.hint":
      "O que o app não consegue ler neste arquivo. Extração curta não prova que o jogo tem pouco texto: ele pode estar comprimido ou numa tabela de caracteres própria.",
    "xray.relocatable":
      "Strings com ponteiro em tabela: {n} de {total}. Nessas a tradução pode passar do tamanho original; as outras precisam caber no espaço que já ocupam.",
    "xray.noEntries":
      "Nenhuma string extraída ainda. Rode a extração estruturada para ver quantas traduções podem passar do tamanho original.",
    "xray.lz77":
      "Blocos comprimidos (LZ77): {blocks}, com {packed} guardando {unpacked}. Trechos com cara de texto dentro deles: cerca de {strings}, em {withText} blocos. No Game Boy Advance a extração estruturada lê os blocos que são mesmo texto; o resto costuma ser gráfico.",
    "xray.lz77None":
      "Nenhum bloco LZ77 (tipo 0x10, o do GBA e do DS) encontrado. Outros compressores não são detectados.",
    "xray.lz77Truncated": "A busca parou no teto de trabalho: pode haver mais blocos.",
    "xray.dense":
      "{dense}% do arquivo é denso (comprimido, cifrado, áudio ou vídeo) e {blank}% é vazio. É uma estimativa por entropia, não uma leitura do formato.",
    "relative.title": "Descobrir a tabela de caracteres",
    "relative.hint":
      "Jogo de cartucho raramente usa ASCII. Digite uma palavra que você sabe que aparece no jogo (só letras, 4 ou mais): o app a procura pela distância entre as letras e monta o .tbl. Busque uma palavra em maiúsculas e outra em minúsculas para cobrir os dois casos.",
    "relative.word": "Palavra do jogo",
    "relative.search": "Buscar",
    "relative.none":
      "Nenhuma ocorrência. Tente outra palavra ou o outro caso. O texto também pode estar comprimido: veja o Raio-X.",
    "relative.preview": "Trecho lido com esta tabela",
    "relative.hits": "Ocorrências",
    "relative.use": "Usar esta tabela",
    "relative.ascii": "É ASCII: use a codificação ascii",
    "relative.saved":
      "Tabela gravada em {path}. Agora clique em Escanear. Ela traz só letras e espaço: complete números e pontuação no arquivo.",
    "relative.adopted":
      "Tabela copiada para {path}. É essa cópia que vale daqui em diante: para completar caracteres, edite esse arquivo.",
    "relative.writeBack":
      "A tabela usada no scan fica em tables/ dentro do projeto, e é por ela que a tradução é regravada no jogo. A IA recebe a lista de caracteres da tabela e o espaço de cada frase. Acento que a tabela não tem é gravado como a letra sem acento, com um aviso na frase. O resto que a tabela não tem (pontuação, por exemplo) bloqueia a reinserção: acrescente a linha dele nesse .tbl ou troque o caractere. Tradução maior que o espaço da original também bloqueia e fica marcada com erro: encurte o texto — nada é cortado sozinho.",
    "reinsert.title": "Reinserção",
    "reinsert.hint":
      "Aplica as traduções numa cópia em working/ do projeto — o arquivo original nunca é modificado. Tradução com erro de validação fica de fora: a frase continua como no original e o resto entra.",
    "reinsert.skipped":
      "{n} frases ficaram com o texto original porque a tradução tem erro (não cabe no espaço ou usa caractere que o jogo não tem). Filtre por erro para corrigir e reinserir de novo.",
    "reinsert.run": "Reinserir na cópia de trabalho",
    "reinsert.running": "Reinserindo...",
    "reinsert.done": "Jogo traduzido pronto e verificado. Abra este arquivo no emulador:",
    "reinsert.reveal": "Mostrar o arquivo",
    "reinsert.personal":
      "Este arquivo é para o seu uso. Para compartilhar a tradução, exporte o patch: quem tem o jogo original aplica o patch e chega no mesmo arquivo.",
    "reinsert.applied": "{applied} traduções aplicadas · {kept} mantidas no original",
    "reinsert.relocated": "{n} realocadas, com os ponteiros atualizados",
    "reinsert.packedLeftOut":
      "{n} traduções de blocos comprimidos ficaram de fora: o bloco regravado ficou maior que o original e o jogo não tem tabela de ponteiros para ele mudar de lugar. Encurte o texto desses blocos e reinsira.",
    "reinsert.advanced": "Modo avançado: reinserir mesmo com erros de validação",
    "reinsert.advancedHint":
      "As entries marcadas com erro entram do jeito que estão — placeholder quebrado ou texto que não cabe pode corromper o jogo. Use só se souber o que está fazendo.",
    "reinsert.forced": "{n} entries com erro de validação entraram (modo avançado)",
    "patch.export": "Exportar patch",
    "patch.exporting": "Gerando patch...",
    "patch.done":
      "Patch {format} gerado ({size} bytes) e conferido por round-trip. Distribua o patch — nunca a ROM.",
    "patch.hint":
      "Exige a reinserção feita. Gera o patch + manifest.json + CSV das traduções em exports/.",
    "patch.formatAuto": "Formato: auto",
    "patch.formatHint":
      "Auto: IPS quando cabe (até 16 MiB, sem encolher — máxima compatibilidade); senão BPS (qualquer tamanho, valida CRC do arquivo certo).",
    "common.back": "Voltar",
    "common.error": "Erro",
  },
  "en-US": {
    "app.title": "RomTranslate Studio",
    "app.tagline": "Local-first, AI-assisted game translation toolkit",
    "home.selectFile": "Select game file",
    "home.openProject": "Open project",
    "home.hint":
      "Select or drag into this window a ROM or dump you legally obtained (or a .rtsproj project folder). The file never leaves your computer at this stage.",
    "home.drop": "Drop to open.",
    "home.title": "Drag your game file here",
    "inspect.loading": "Inspecting file...",
    "inspect.title": "Detection result",
    "inspect.platform": "Platform",
    "inspect.adapter": "Adapter",
    "inspect.confidence": "Confidence",
    "inspect.support": "Support",
    "inspect.evidence": "Evidence",
    "inspect.file": "File",
    "inspect.size": "Size",
    "inspect.sha256": "SHA-256",
    "inspect.noMatch":
      "Format not recognized by current adapters. Hash and size are still shown above.",
    "inspect.otherCandidates": "Other candidates",
    "support.full": "Full",
    "support.partial": "Partial",
    "support.extract_only": "Extract only",
    "support.experimental": "Experimental",
    "support.unsupported": "Unsupported",
    "project.create": "Create project",
    "project.otherGame":
      "A project already exists at {dir}, but for a different game file (the content differs). Open it with \"Open project\", or move or rename that folder to create a new one.",
    "project.creating": "Creating project...",
    "project.sourceLanguage": "Source language",
    "project.targetLanguage": "Target language",
    "project.auto": "Detect later",
    "project.createdTitle": "Project created",
    "project.createdAt": "Project saved at:",
    "project.extract": "Extract strings",
    "project.sourceMissing":
      "The source ROM is no longer at the path stored in the project. Put the file back and reopen.",
    "project.sourceChanged":
      "Warning: the source file changed since the project was created (different SHA-256).",
    "extract.title": "String extraction",
    "extract.hint":
      "To translate and write the game back, use Structured extraction: it records the space of each line and drops what is not text. Scan is the generic scanner, for discovery.",
    "extract.encoding": "Encoding",
    "extract.minChars": "Min. characters",
    "extract.chooseTbl": "Choose .tbl",
    "extract.tblMissing": "Pick a .tbl file to scan with a custom table.",
    "extract.scan": "Scan",
    "extract.scanning": "Scanning...",
    "extract.found": "{n} strings found",
    "extract.truncated": "the scan stopped at its limit: the file has more sequences than these.",
    "extract.onlyText": "Only what looks like text",
    "extract.onlyTextHint":
      "Drops tiles, code and tables that happen to be printable bytes. Applies to the Scan button; untick it to see everything.",
    "extract.discarded":
      "{n} sequences dropped because they do not look like text. To see everything, untick “Only what looks like text” and scan again. Whatever was already in the project stays in the list.",
    "extract.discardedStructured":
      "{n} sequences dropped because they do not look like text. Structured extraction always filters, because what it returns is written back into the game; a string with a pointer in a table is never dropped. To see everything, use Scan with “Only what looks like text” unticked.",
    "extract.removedOld":
      "{n} old strings with no translation left the project because they do not look like text. Whatever has a translation, a pointer in a table or was read through a character table stayed.",
    "extract.none": "No strings with this configuration.",
    "extract.emptyTitle": "No text extracted yet",
    "extract.emptyHint":
      "Click “Structured extraction” to pull the game's text. It shows up here, ready to translate.",
    "extract.filter": "Filter text...",
    "extract.showing": "Showing {shown} of {total}",
    "extract.offset": "Offset",
    "extract.bytes": "Bytes",
    "extract.text": "Text",
    "export.json": "Export JSON",
    "export.csv": "Export CSV",
    "export.saved": "Saved to: {path}",
    "entries.saved": "{n} strings saved to the project",
    "table.translation": "Translation",
    "table.resource": "File",
    "status.untranslated": "pending",
    "status.machine": "machine",
    "status.reviewed": "reviewed",
    "status.error": "error",
    "status.internal": "internal name",
    "translate.title": "Translation",
    "translate.provider": "Provider",
    "translate.baseUrl": "Base URL",
    "translate.model": "Model",
    "translate.apiKey": "API key",
    "translate.apiKeySet": "(saved — leave empty to keep)",
    "translate.allowRemote": "Allow remote translation (sends the TEXTS to the configured endpoint)",
    "translate.codexCommand": "Codex command",
    "translate.codexModel": "Model (optional)",
    "translate.codexModelDefault": "Codex default",
    "translate.codexHint":
      "Uses your ChatGPT subscription through the Codex CLI, with no API key. The app never sees your password or token. The game text is sent to OpenAI, so remote translation must be allowed. Each batch is one Codex call: a large project uses up the subscription limit.",
    "translate.codexAccount": "Account",
    "translate.codexAccountSystem": "The one this computer's Codex uses",
    "translate.codexAccountApp": "An account just for this app",
    "translate.codexLogin": "Sign in with ChatGPT",
    "translate.codexSwitch": "Switch account",
    "translate.codexLogout": "Sign out",
    "translate.codexLoginCancel": "Cancel sign-in",
    "translate.codexLoginWaiting":
      "Finish signing in on the page that opened in your browser: that is where you pick the account. The app waits up to 5 minutes.",
    "translate.codexChecking": "Checking sign-in...",
    "translate.codexLogged": "Account connected to ChatGPT.",
    "translate.codexNotLoggedApp":
      "This account has no sign-in yet. Click \"Sign in with ChatGPT\" and pick the account in your browser.",
    "translate.codexNotLoggedSystem":
      "This computer's Codex is not signed in. Run `codex login` in a terminal, or choose \"An account just for this app\".",
    "translate.saveSettings": "Save settings",
    "translate.remoteAsk":
      "To use this provider, the game text is sent off your computer (to OpenAI, in the case of ChatGPT). The game file itself is not sent. Allow?",
    "translate.remoteAllow": "Allow and continue",
    "translate.providerClaudeKey": "Claude (API key)",
    "translate.claudeKeyHint":
      "Translates with Claude through the Anthropic API, using your own API key (created in the Anthropic console). The key lives in the system keychain, never in the project. The game text is sent to Anthropic, and usage is billed per token to your account.",
    "translate.priceInput": "US$ per million tokens (input)",
    "translate.priceOutput": "US$ per million tokens (output)",
    "translate.priceHint":
      "Your model's price, for the spend estimate. Leave 0 to use the table of Claude models the app knows.",
    "translate.priceUsed":
      "Calculated with US$ {input} (input) and US$ {output} (output) per million tokens. Check the provider's price list: the app does not update it by itself.",
    "translate.priceMissing":
      "No price set: the app shows tokens only. Fill both fields to see the amount.",
    "translate.estimate": "Estimate spend",
    "translate.estimating": "Estimating...",
    "translate.estimateResult":
      "Translating the {entries} pending lines takes {batches} calls: about {input} input tokens and {output} output tokens.",
    "translate.estimateCost": "Estimated cost: US$ {cost}.",
    "translate.estimateCalibrated":
      "Input counted by the model itself. Output is an estimate: a model that reasons before answering spends more output than this. Translation memory may lower the total.",
    "translate.estimateRough":
      "Estimated from an average of characters per token, with a wide margin. A model that reasons before answering spends more output. Translation memory may lower the total.",
    "translate.spent": "Billed this round: {input} input tokens and {output} output tokens.",
    "translate.spentCost": "About US$ {cost}.",
    "translate.batchSize": "Lines per call",
    "translate.batchSizeHint":
      "How many lines go in each call to the AI (1 to 50). More lines per call means fewer calls, which weighs less on a subscription limit; if answers start failing, lower it.",
    "translate.stopped":
      "Translation stopped because the provider stopped answering: {error} — what was translated is saved and the rest is still pending.",
    "translate.test": "Test connection",
    "translate.testing": "Testing...",
    "translate.testOk": "Connection OK — model: {model}",
    "translate.run": "Translate pending",
    "translate.running": "Translating...",
    "translate.cancel": "Cancel",
    "translate.phase.tm": "translation memory",
    "translate.phase.translate": "translating",
    "translate.phase.shorten": "shortening the ones that did not fit",
    "translate.summaryShortened": " · {shortened} shortened to fit",
    "translate.summaryTooLong": " · {tooLong} still too long",
    "translate.summary": "{translated} translated · {tmHits} from memory · {failed} failed",
    "translate.summaryCancelled": " · cancelled",
    "translate.internal":
      "{n} strings are left out of the translation: internal names of the game (room, script: only capitals, digits and underscores, no space) and stray pieces outside the text regions, which are usually graphics or sound data. They are not sent to the AI and are not written back. To see them, filter by “internal names”; if one is game text, select it and click “It is game text”.",
    "translate.cancelling": "Cancelling…",
    "translate.saved": "Settings saved.",
    "extract.stray":
      "{n} strings are outside the game's text regions and left the translation queue (any machine translation they had was discarded, so it is not written over game data). They show up under the “internal names” filter.",
    "extract.packed":
      "{n} lines were inside compressed blocks (LZ77). Each translation must fit the space of its original; on reinsertion the block is compressed again.",
    "translate.ascii":
      "ASCII text has no accented letters: the AI is told so and, if it still answers with an accent or with typographic quotes and dashes, the app writes the plain letter and the plain sign, with a warning on the string. Anything with no equivalent blocks reinsertion and the string says which character it is.",
    "glossary.title": "Glossary",
    "glossary.term": "Term",
    "glossary.translation": "Translation",
    "glossary.noTranslate": "Do not translate",
    "glossary.add": "Add",
    "glossary.remove": "Remove",
    "glossary.empty": "No terms yet. Terms guide the AI (e.g. Potion → Poção; HP → do not translate).",
    "filter.all": "all",
    "filter.untranslated": "pending",
    "filter.machine": "machine translated",
    "filter.reviewed": "reviewed",
    "filter.error": "with errors",
    "filter.internal": "internal names",
    "filter.too_long": "too long",
    "filter.placeholder_mismatch": "placeholder mismatch",
    "validate.run": "Validate",
    "validate.summary": "{errors} errors · {warnings} warnings across {checked} translated",
    "editor.pick": "Click a table row to edit its translation.",
    "editor.original": "Original",
    "editor.translation": "Translation",
    "editor.bytes": "{n} bytes",
    "editor.bytesLimit": "limit {max}",
    "editor.bytesOriginal": "original {n}",
    "editor.relocatable": "relocatable — may exceed the original",
    "editor.noEncoder": "no byte count for this encoding",
    "editor.save": "Save translation",
    "editor.markReviewed": "Mark reviewed",
    "editor.unmarkReviewed": "Unmark review",
    "editor.internal":
      "Internal name of the game: left out of the translation and not written back. The mark comes back if you extract again before translating.",
    "editor.unmarkInternal": "It is game text",
    "extract.structured": "Structured extraction",
    "extract.xray": "X-ray",
    "xray.title": "File X-ray",
    "xray.hint":
      "What the app cannot read in this file. A short extraction does not prove the game has little text: it may be compressed or use its own character table.",
    "xray.relocatable":
      "Strings with a pointer in a table: {n} of {total}. For those the translation may be longer than the original; the others must fit the space they already use.",
    "xray.noEntries":
      "No strings extracted yet. Run the structured extraction to see how many translations may outgrow the original.",
    "xray.lz77":
      "Compressed blocks (LZ77): {blocks}, with {packed} holding {unpacked}. Text-looking pieces inside them: about {strings}, in {withText} blocks. On Game Boy Advance the structured extraction reads the blocks that really are text; the rest is usually graphics.",
    "xray.lz77None":
      "No LZ77 block (type 0x10, the GBA and DS one) found. Other compressors are not detected.",
    "xray.lz77Truncated": "The search stopped at its work limit: there may be more blocks.",
    "xray.dense":
      "{dense}% of the file is dense (compressed, encrypted, audio or video) and {blank}% is empty. This is an entropy estimate, not a reading of the format.",
    "relative.title": "Find the character table",
    "relative.hint":
      "Cartridge games rarely use ASCII. Type a word you know appears in the game (letters only, 4 or more): the app finds it by the distance between its letters and builds the .tbl. Search one uppercase word and one lowercase word to cover both cases.",
    "relative.word": "Word from the game",
    "relative.search": "Search",
    "relative.none":
      "No match. Try another word or the other case. The text may also be compressed: see the X-ray.",
    "relative.preview": "Excerpt read with this table",
    "relative.hits": "Matches",
    "relative.use": "Use this table",
    "relative.ascii": "It is ASCII: use the ascii encoding",
    "relative.saved":
      "Table saved to {path}. Now click Scan. It only has letters and the space: add digits and punctuation to the file.",
    "relative.adopted":
      "Table copied to {path}. That copy is the one that counts from now on: to add characters, edit that file.",
    "relative.writeBack":
      "The table used for the scan is kept in tables/ inside the project, and the translation is written back into the game through it. The AI is given the table's list of characters and the room each string has. An accent the table lacks is written as the plain letter, with a warning on the string. Anything else the table lacks (punctuation, for example) blocks reinsertion: add its line to that .tbl or change the character. A translation larger than the original's space also blocks it and is flagged as an error: shorten the text — nothing is cut automatically.",
    "reinsert.title": "Reinsertion",
    "reinsert.hint":
      "Applies translations to a copy under the project's working/ folder — the original file is never modified. A translation with a validation error is left out: that line stays as in the original and the rest goes in.",
    "reinsert.skipped":
      "{n} lines kept the original text because their translation has an error (it does not fit, or uses a character the game lacks). Filter by error to fix them and reinsert again.",
    "reinsert.run": "Reinsert into working copy",
    "reinsert.running": "Reinserting...",
    "reinsert.done": "Translated game ready and verified. Open this file in your emulator:",
    "reinsert.reveal": "Show the file",
    "reinsert.personal":
      "This file is for your own use. To share the translation, export the patch: anyone who owns the original game applies it and gets the same file.",
    "reinsert.applied": "{applied} translations applied · {kept} kept original",
    "reinsert.relocated": "{n} relocated, with pointers updated",
    "reinsert.packedLeftOut":
      "{n} translations in compressed blocks were left out: the rewritten block came out larger than the original and the game has no pointer table that lets it move. Shorten the text in those blocks and reinsert.",
    "reinsert.advanced": "Advanced mode: reinsert even with validation errors",
    "reinsert.advancedHint":
      "Entries flagged with errors go in as-is — broken placeholders or oversized text may corrupt the game. Use only if you know what you are doing.",
    "reinsert.forced": "{n} entries with validation errors were forced in (advanced mode)",
    "patch.export": "Export patch",
    "patch.exporting": "Building patch...",
    "patch.done":
      "{format} patch built ({size} bytes) and round-trip checked. Distribute the patch — never the ROM.",
    "patch.hint":
      "Requires a completed reinsertion. Writes the patch + manifest.json + translations CSV into exports/.",
    "patch.formatAuto": "Format: auto",
    "patch.formatHint":
      "Auto: IPS when it fits (up to 16 MiB, no shrinking — maximum compatibility); otherwise BPS (any size, validates the right source via CRC).",
    "common.back": "Back",
    "common.error": "Error",
  },
} as const;

export type MessageId = keyof (typeof messages)["pt-BR"];

export function detectLocale(): Locale {
  return navigator.language?.toLowerCase().startsWith("pt") ? "pt-BR" : "en-US";
}

export const dictionaries = messages;

export function makeT(locale: Locale) {
  return (id: MessageId, params?: Record<string, string | number>): string => {
    let msg: string = messages[locale][id] ?? id;
    if (params) {
      for (const [key, value] of Object.entries(params)) {
        msg = msg.replace(`{${key}}`, String(value));
      }
    }
    return msg;
  };
}
