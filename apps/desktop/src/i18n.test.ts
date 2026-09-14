import { describe, expect, it } from "vitest";
import { dictionaries, makeT } from "./i18n";

describe("i18n", () => {
  it("pt-BR e en-US tem exatamente as mesmas chaves", () => {
    const pt = Object.keys(dictionaries["pt-BR"]).sort();
    const en = Object.keys(dictionaries["en-US"]).sort();
    expect(en).toEqual(pt);
  });

  it("nenhuma mensagem vazia", () => {
    for (const dict of Object.values(dictionaries)) {
      for (const [id, msg] of Object.entries(dict)) {
        expect(msg, id).not.toBe("");
      }
    }
  });

  it("t() resolve mensagens", () => {
    expect(makeT("pt-BR")("project.create")).toBe("Criar projeto");
    expect(makeT("en-US")("project.create")).toBe("Create project");
  });
});
