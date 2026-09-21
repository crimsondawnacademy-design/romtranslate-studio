export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB"];
  let value = bytes;
  let unit = "B";
  for (const u of units) {
    if (value < 1024) break;
    value /= 1024;
    unit = u;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${unit}`;
}

import { TextEncodingWire, TextEntry } from "./types";

/** Bytes da string no encoding destino; null = nao codificavel ou sem encoder. */
export function encodedByteLength(
  text: string,
  encoding: TextEncodingWire,
): number | null {
  if (typeof encoding === "object") return null; // tabela custom: sem encoder no front
  switch (encoding) {
    case "ascii":
      // eslint-disable-next-line no-control-regex
      return /^[\x00-\x7F]*$/.test(text) ? text.length : null;
    case "utf8":
      return new TextEncoder().encode(text).length;
    case "utf16_le":
    case "utf16_be": {
      let units = 0;
      for (const ch of text) units += (ch.codePointAt(0) ?? 0) > 0xffff ? 2 : 1;
      return units * 2;
    }
    case "shift_jis":
      return null;
  }
}

/** Ponteiros em tabela que o adapter achou pra string (0 = so in-place). */
export function pointerCount(entry: TextEntry): number {
  const pointers = (entry.metadata as { pointers?: unknown } | null)?.pointers;
  return Array.isArray(pointers) ? pointers.length : 0;
}

export function formatOffset(offset: number | null): string {
  if (offset === null) return "";
  return "0x" + offset.toString(16).toUpperCase().padStart(8, "0");
}

/** `/roms/Game.gba` -> `/roms/Game.rtsproj` (aceita separador Windows). */
export function defaultProjectDir(sourcePath: string): string {
  const sep = sourcePath.includes("\\") ? "\\" : "/";
  const slash = sourcePath.lastIndexOf(sep);
  const dir = slash >= 0 ? sourcePath.slice(0, slash + 1) : "";
  const name = slash >= 0 ? sourcePath.slice(slash + 1) : sourcePath;
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  return `${dir}${stem}.rtsproj`;
}
