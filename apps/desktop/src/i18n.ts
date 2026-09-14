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
      "Selecione uma ROM ou dump obtido legalmente por você. O arquivo nunca sai do seu computador nesta etapa.",
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
      "Formato não reconhecido pelos adapters atuais (GBA, NES, SNES). Você ainda pode ver hash e tamanho acima.",
    "inspect.otherCandidates": "Outros candidatos",
    "support.full": "Completo",
    "support.partial": "Parcial",
    "support.extract_only": "Somente extração",
    "support.experimental": "Experimental",
    "support.unsupported": "Não suportado",
    "project.create": "Criar projeto",
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
      "Scanner genérico: serve para descoberta e debug — não garante reinserção segura.",
    "extract.encoding": "Codificação",
    "extract.minChars": "Mín. caracteres",
    "extract.chooseTbl": "Escolher .tbl",
    "extract.tblMissing": "Escolha um arquivo .tbl para escanear com tabela.",
    "extract.scan": "Escanear",
    "extract.scanning": "Escaneando...",
    "extract.found": "{n} strings encontradas",
    "extract.truncated":
      "Resultado cortado em {n} entries — restrinja a região ou aumente o limite.",
    "extract.none": "Nenhuma string com essa configuração.",
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
    "status.untranslated": "pendente",
    "status.machine": "máquina",
    "status.reviewed": "revisada",
    "status.error": "erro",
    "translate.title": "Tradução",
    "translate.provider": "Provider",
    "translate.baseUrl": "Base URL",
    "translate.model": "Modelo",
    "translate.apiKey": "API key",
    "translate.apiKeySet": "(já salva — deixe vazio para manter)",
    "translate.allowRemote": "Permitir tradução remota (envia os TEXTOS ao endpoint configurado)",
    "translate.saveSettings": "Salvar configuração",
    "translate.test": "Testar conexão",
    "translate.testOk": "Conexão OK — modelo: {model}",
    "translate.run": "Traduzir pendentes",
    "translate.running": "Traduzindo...",
    "translate.cancel": "Cancelar",
    "translate.phase.tm": "memória de tradução",
    "translate.phase.translate": "traduzindo",
    "translate.summary":
      "{translated} traduzidas · {tmHits} da memória · {failed} falharam",
    "translate.summaryCancelled": " · cancelado",
    "glossary.title": "Glossário",
    "glossary.term": "Termo",
    "glossary.translation": "Tradução",
    "glossary.noTranslate": "Não traduzir",
    "glossary.add": "Adicionar",
    "glossary.remove": "Remover",
    "glossary.empty": "Nenhum termo ainda. Termos guiam a IA (ex.: Potion → Poção; HP → não traduzir).",
    "common.back": "Voltar",
    "common.error": "Erro",
  },
  "en-US": {
    "app.title": "RomTranslate Studio",
    "app.tagline": "Local-first, AI-assisted game translation toolkit",
    "home.selectFile": "Select game file",
    "home.openProject": "Open project",
    "home.hint":
      "Select a ROM or dump you legally own. The file never leaves your computer in this step.",
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
      "Format not recognized by current adapters (GBA, NES, SNES). Hash and size are still shown above.",
    "inspect.otherCandidates": "Other candidates",
    "support.full": "Full",
    "support.partial": "Partial",
    "support.extract_only": "Extract only",
    "support.experimental": "Experimental",
    "support.unsupported": "Unsupported",
    "project.create": "Create project",
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
      "Generic scanner: discovery and debugging — it does not guarantee safe reinsertion.",
    "extract.encoding": "Encoding",
    "extract.minChars": "Min. characters",
    "extract.chooseTbl": "Choose .tbl",
    "extract.tblMissing": "Pick a .tbl file to scan with a custom table.",
    "extract.scan": "Scan",
    "extract.scanning": "Scanning...",
    "extract.found": "{n} strings found",
    "extract.truncated":
      "Result cut at {n} entries — narrow the region or raise the limit.",
    "extract.none": "No strings with this configuration.",
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
    "status.untranslated": "pending",
    "status.machine": "machine",
    "status.reviewed": "reviewed",
    "status.error": "error",
    "translate.title": "Translation",
    "translate.provider": "Provider",
    "translate.baseUrl": "Base URL",
    "translate.model": "Model",
    "translate.apiKey": "API key",
    "translate.apiKeySet": "(saved — leave empty to keep)",
    "translate.allowRemote": "Allow remote translation (sends the TEXTS to the configured endpoint)",
    "translate.saveSettings": "Save settings",
    "translate.test": "Test connection",
    "translate.testOk": "Connection OK — model: {model}",
    "translate.run": "Translate pending",
    "translate.running": "Translating...",
    "translate.cancel": "Cancel",
    "translate.phase.tm": "translation memory",
    "translate.phase.translate": "translating",
    "translate.summary": "{translated} translated · {tmHits} from memory · {failed} failed",
    "translate.summaryCancelled": " · cancelled",
    "glossary.title": "Glossary",
    "glossary.term": "Term",
    "glossary.translation": "Translation",
    "glossary.noTranslate": "Do not translate",
    "glossary.add": "Add",
    "glossary.remove": "Remove",
    "glossary.empty": "No terms yet. Terms guide the AI (e.g. Potion → Poção; HP → do not translate).",
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
