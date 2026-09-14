// Espelho TS dos tipos serde do core (rename_all = camelCase).

export type Platform =
  | "nes"
  | "snes"
  | "gba"
  | "nds"
  | "game_cube"
  | "wii"
  | "wii_u"
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

export const PLATFORM_NAMES: Record<Platform, string> = {
  nes: "Nintendo Entertainment System",
  snes: "Super Nintendo",
  gba: "Game Boy Advance",
  nds: "Nintendo DS",
  game_cube: "GameCube",
  wii: "Wii",
  wii_u: "Wii U",
  unknown: "?",
};
