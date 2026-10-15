// Espelho TS dos tipos serde do core (rename_all = camelCase).

export type Platform =
  | "nes"
  | "snes"
  | "gba"
  | "nds"
  | "game_cube"
  | "wii"
  | "wii_u"
  | "ps1"
  | "ps2"
  | "psp"
  | "synthetic"
  | "unknown";

export type SupportLevel =
  | "full"
  | "partial"
  | "extract_only"
  | "experimental"
  | "unsupported";

export interface ProbeResult {
  adapterId: string;
  platform: Platform;
  confidence: number;
  evidence: string[];
  supportLevel: SupportLevel;
}

export interface InspectionReport {
  path: string;
  size: number;
  sha256: string;
  results: ProbeResult[];
  best: ProbeResult | null;
}

export interface GameProject {
  id: string;
  sourcePath: string;
  sourceSha256: string;
  sourceSize: number;
  platform: Platform;
  adapterId: string;
  sourceLanguage: string | null;
  targetLanguage: string;
  status: string;
  createdAt: string;
}

export type TextEncodingWire =
  | "ascii"
  | "utf8"
  | "utf16_le"
  | "utf16_be"
  | "shift_jis"
  | "image"
  | { table: string };

export interface TextEntry {
  id: string;
  resourcePath: string | null;
  offset: number | null;
  /** hex string (serde serializa bytes como hex) */
  originalBytes: string;
  sourceText: string;
  translatedText: string | null;
  context: string | null;
  maxBytes: number | null;
  encoding: TextEncodingWire;
  status: string;
  metadata: unknown;
}

export type ScanEncoding = "ascii" | "utf8" | "utf16_le" | "utf16_be" | "shift_jis" | "table";

export interface ScanConfig {
  encoding: ScanEncoding;
  tblPath: string | null;
  minChars: number;
  regionStart: number | null;
  regionEnd: number | null;
  /** Sem valor, vale o teto do core. */
  maxEntries?: number;
  /** Descarta o que nao parece texto, antes do teto. */
  onlyText: boolean;
}

export interface ScanOutcome {
  entries: TextEntry[];
  truncated: boolean;
  scannedBytes: number;
  /** Sequencias que o filtro descartou por nao parecerem texto. */
  discarded: number;
  /** Tabela que o scan leu: a copia em tables/ do projeto. */
  tblPath: string | null;
}

/** Extracao estruturada: sempre filtra o que nao parece texto. */
export interface ExtractOutcome {
  saved: number;
  discarded: number;
  truncated: boolean;
  /** Entries antigas sem traducao que sairam do projeto por nao parecerem texto. */
  removed: number;
  /** Fora das regiões de texto do arquivo: tiradas da tradução. */
  stray: number;
  /** Das salvas, quantas estavam dentro de blocos comprimidos. */
  packed: number;
  /** Das salvas, quantas sao japones em codigos de 16 bits, lido por tabela gerada. */
  jis16: number;
  /** Das salvas, quantas estao num banco em blocos: a traducao pode crescer. */
  banked: number;
  /** Traducoes ja feitas relidas com os comandos na forma da leitura de agora. */
  reread: number;
}

export interface OpenProjectReport {
  project: GameProject;
  sourceFound: boolean;
  sourceChanged: boolean;
}

export interface GlossaryTerm {
  term: string;
  translation: string | null;
  noTranslate: boolean;
  caseSensitive: boolean;
  note: string | null;
}

export interface TranslateSummary {
  translated: number;
  tmHits: number;
  failed: number;
  cancelled: boolean;
  /** O provider parou de responder: o erro dele. A rodada foi interrompida. */
  stopped: string | null;
  /** Não cabiam no espaço da frase e passaram a caber depois de repedidas. */
  shortened: number;
  /** Traduções desta rodada que continuam maiores que o espaço da frase. */
  tooLong: number;
  /** Tokens que o provider declarou ter cobrado na rodada (0 = não informa). */
  inputTokens: number;
  outputTokens: number;
}

/** Tamanho da rodada que "Traduzir pendentes" faria agora. */
export interface EstimateReport {
  entries: number;
  batches: number;
  inputTokens: number;
  outputTokens: number;
  /** true = contado pelo próprio modelo; false = média de caracteres por token. */
  calibrated: boolean;
}

export interface ProgressEvent {
  done: number;
  total: number;
  phase: "tm" | "translate";
}

export interface EndpointSettings {
  baseUrl: string;
  model: string;
}

export interface CodexCliSettings {
  /** Nome ou caminho do executável do Codex CLI. */
  command: string;
  /** Vazio = o modelo padrão do Codex. */
  model: string;
  /** Modelo pra ler texto desenhado em imagem. Vazio = o padrão do Codex. */
  imageModel: string;
  /** true = login só do app; false = a conta do Codex deste computador. */
  ownAccount: boolean;
}

export interface AppSettings {
  provider: "ollama" | "openai_compatible" | "anthropic" | "codex_cli";
  ollama: EndpointSettings;
  openaiCompatible: EndpointSettings;
  anthropic: EndpointSettings;
  codexCli: CodexCliSettings;
  /** US$ por milhão de tokens; 0 = não informado. */
  priceInput: number;
  priceOutput: number;
  allowRemoteTranslation: boolean;
  batchSize: number;
  timeoutSecs: number;
}

export interface SettingsReport {
  settings: AppSettings;
  apiKeySet: boolean;
  anthropicKeySet: boolean;
}

/**
 * Preço de tabela da API da Anthropic, em US$ por milhão de tokens
 * [entrada, saída], lido em 06/10/2026. Serve de padrão quando a pessoa não
 * informa o preço; a tabela oficial é a que vale.
 */
export const CLAUDE_PRICES: Record<string, [number, number]> = {
  "claude-fable-5-1": [10, 50],
  "claude-fable-5": [10, 50],
  "claude-opus-5-5": [4, 20],
  "claude-opus-5": [5, 25],
  "claude-opus-4-8": [5, 25],
  "claude-opus-4-7": [5, 25],
  "claude-opus-4-6": [5, 25],
  "claude-sonnet-5-5": [2, 10],
  "claude-sonnet-5": [2, 10],
  "claude-sonnet-4-6": [3, 15],
  "claude-haiku-5-5": [0.1, 0.5],
  "claude-haiku-4-5": [1, 5],
};

export type Severity = "error" | "warning";

export type IssueKind =
  | "placeholder_mismatch"
  | "byte_overflow"
  | "empty_translation"
  | "length_anomaly"
  | "unencodable"
  | "accent_dropped";

export interface ValidationIssue {
  entryId: string;
  severity: Severity;
  kind: IssueKind;
  message: string;
}

export interface ValidationReport {
  issues: ValidationIssue[];
  errors: number;
  warnings: number;
  checked: number;
}

export const PLATFORM_NAMES: Record<Platform, string> = {
  nes: "Nintendo Entertainment System",
  snes: "Super Nintendo",
  gba: "Game Boy Advance",
  nds: "Nintendo DS",
  game_cube: "GameCube",
  wii: "Wii",
  wii_u: "Wii U",
  ps1: "PlayStation",
  ps2: "PlayStation 2",
  psp: "PSP",
  synthetic: "Fixture sintética (RTSF)",
  unknown: "?",
};

export interface ApplyReport {
  applied: number;
  keptOriginal: number;
  ignoredGeneric: number;
  relocated: number;
  /** Traducoes de bloco comprimido que nao couberam: o bloco ficou como no original. */
  packedLeftOut: number;
  /** Traducoes de mensagem de banco que ficaram sem espaco no arquivo. */
  noRoom: number;
}

export interface VerificationReport {
  ok: boolean;
  checks: string[];
  problems: string[];
}

export interface ReinsertOutcome {
  workingPath: string;
  apply: ApplyReport;
  verification: VerificationReport;
  forcedErrors: number;
  /** Traduções com erro que ficaram de fora: a frase segue como no original. */
  skippedErrors: number;
  /** Texto desenhado em imagem: imagens regravadas e frases que ficaram no original. */
  pictures: PicturesApplied;
}

export interface PicturesApplied {
  pictures: number;
  drawn: number;
  /** [id da frase, por que ficou no original]. */
  leftOut: [string, string][];
}

/** Uma imagem do arquivo com quadros que parecem letra. */
export interface Picture {
  id: string;
  offset: number;
  /** Bytes do bloco comprimido (LZ77); null = quadros crus. */
  packed: number | null;
  /** Bytes dos quadros, já descomprimidos. */
  bytes: number;
  /** Quadros que parecem letra. */
  lettered: number;
}

export interface ReadPicturesOutcome {
  pictures: number;
  blocks: number;
  entries: number;
  /** [id da imagem, por que não foi lida]. */
  failed: [string, string][];
}

export interface PatchExportOutcome {
  patchPath: string;
  manifestPath: string;
  csvPath: string;
  patchFormat: string;
  patchedSha256: string;
  patchSize: number;
}

export interface RelativeCandidate {
  /** Byte de 'A'; null = a palavra buscada não tinha maiúscula. */
  upperBase: number | null;
  lowerBase: number | null;
  hits: number;
  offsets: number[];
  preview: string;
  ascii: boolean;
}

export interface Lz77Block {
  offset: number;
  packedLen: number;
  unpackedLen: number;
  strings: number;
}

export interface Lz77Summary {
  blocks: number;
  packedBytes: number;
  unpackedBytes: number;
  blocksWithText: number;
  strings: number;
  truncated: boolean;
  first: Lz77Block[];
}

export interface XrayReport {
  size: number;
  denseBytes: number;
  blankBytes: number;
  lz77: Lz77Summary;
  entries: number;
  relocatable: number;
}
