import { describe, expect, it } from "vitest";
import { defaultProjectDir, formatBytes } from "./util";

describe("formatBytes", () => {
  it("formata unidades binarias", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1024)).toBe("1.0 KiB");
    expect(formatBytes(16 * 1024 * 1024)).toBe("16.0 MiB");
    expect(formatBytes(200 * 1024)).toBe("200 KiB");
  });
});

describe("defaultProjectDir", () => {
  it("troca a extensao por .rtsproj mantendo o diretorio", () => {
    expect(defaultProjectDir("/roms/Game.gba")).toBe("/roms/Game.rtsproj");
    expect(defaultProjectDir("/a/b/x.y.sfc")).toBe("/a/b/x.y.rtsproj");
    expect(defaultProjectDir("Game.nes")).toBe("Game.rtsproj");
    expect(defaultProjectDir("noext")).toBe("noext.rtsproj");
  });

  it("aceita separador Windows", () => {
    expect(defaultProjectDir("C:\\roms\\Game.smc")).toBe("C:\\roms\\Game.rtsproj");
  });
});
