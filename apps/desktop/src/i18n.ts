// i18n minimo por IDs de mensagem (spec §29). Sem lib: um dict por locale.
// Quando crescer, migrar para uma lib de i18n mantendo os mesmos IDs.

export type Locale = "pt-BR" | "en-US";

const messages = {
  "pt-BR": {
    "app.title": "RomTranslate Studio",
    "app.tagline": "Tradução de jogos assistida por IA — local-first",
    "home.selectFile": "Selecionar arquivo de jogo",
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
    "project.next":
      "Próximo passo (Sprint 2): extração de texto. Por enquanto o projeto guarda caminho, hash e configuração de idiomas.",
    "common.back": "Voltar",
    "common.error": "Erro",
  },
  "en-US": {
    "app.title": "RomTranslate Studio",
    "app.tagline": "Local-first, AI-assisted game translation toolkit",
    "home.selectFile": "Select game file",
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
    "project.next":
      "Next step (Sprint 2): text extraction. For now the project stores path, hash and language settings.",
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
  return (id: MessageId): string => messages[locale][id] ?? id;
}
