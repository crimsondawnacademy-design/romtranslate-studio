import { describe, expect, it } from "vitest";
import { defaultProjectDir, encodedByteLength, formatBytes, formatOffset } from "./util";

describe("encodedByteLength", () => {
  it("calcula por encoding e devolve null quando nao codifica", () => {
    expect(encodedByteLength("SAVE", "ascii")).toBe(4);
    expect(encodedByteLength("POÇÃO", "ascii")).toBeNull();
    expect(encodedByteLength("Poção", "utf8")).toBe(7);
    expect(encodedByteLength("ABC", "utf16_le")).toBe(6);
    expect(encodedByteLength("𝄞", "utf16_be")).toBe(4); // par surrogate
    expect(encodedByteLength("abc", { table: "x" })).toBeNull();
    expect(encodedByteLength("abc", "shift_jis")).toBeNull();
  });
});

describe("formatOffset", () => {
  it("hex de 8 digitos ou vazio", () => {
    expect(formatOffset(0)).toBe("0x00000000");
    expect(formatOffset(0x1fc0)).toBe("0x00001FC0");
    expect(formatOffset(null)).toBe("");
  });
});

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
