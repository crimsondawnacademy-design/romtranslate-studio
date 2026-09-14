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
