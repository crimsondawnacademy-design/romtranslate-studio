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

export type ScanEncoding = "ascii" | "utf8" | "utf16_le" | "utf16_be" | "table";

export interface ScanConfig {
  encoding: ScanEncoding;
  tblPath: string | null;
  minChars: number;
  regionStart: number | null;
  regionEnd: number | null;
  maxEntries: number;
}

export interface ScanOutcome {
  entries: TextEntry[];
  truncated: boolean;
  scannedBytes: number;
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

export interface AppSettings {
  provider: "ollama" | "openai_compatible";
  ollama: EndpointSettings;
  openaiCompatible: EndpointSettings;
  allowRemoteTranslation: boolean;
  batchSize: number;
  timeoutSecs: number;
}

export interface SettingsReport {
  settings: AppSettings;
  apiKeySet: boolean;
}

export type Severity = "error" | "warning";

export type IssueKind =
  | "placeholder_mismatch"
  | "byte_overflow"
  | "empty_translation"
  | "length_anomaly"
  | "unencodable";

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
}

export interface PatchExportOutcome {
  patchPath: string;
  manifestPath: string;
  csvPath: string;
  patchFormat: string;
  patchedSha256: string;
  patchSize: number;
}
