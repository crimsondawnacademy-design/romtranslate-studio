import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { makeT } from "./i18n";
import { parseHex } from "./util";
import { Alphabet } from "./types";

const hex4 = (command: number) => command.toString(16).toUpperCase().padStart(4, "0");

/**
 * Alfabeto que vale depois de um comando do jogo (`tables/jis16@<COMANDO>.tbl`):
 * quem olhou a fonte no emulador escreve as letras dos códigos 1, 2, 3... e
 * o app extrai de novo, lendo as falas escritas com ela.
 */
export default function AlphabetPanel({
  projectDir,
  t,
  disabled,
  onSaved,
}: {
  projectDir: string;
  t: ReturnType<typeof makeT>;
  disabled: boolean;
  /** Extrair de novo, já com o alfabeto. */
  onSaved: () => Promise<void>;
}) {
  const [alphabets, setAlphabets] = useState<Alphabet[]>([]);
  const [command, setCommand] = useState("");
  const [letters, setLetters] = useState("");
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setAlphabets(await invoke<Alphabet[]>("list_alphabets", { projectDir }));
    } catch (e) {
      setError(String(e));
    }
  }, [projectDir]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function save() {
    setError(null);
    setSaved(null);
    const code = parseHex(command.replace(/[[\]]/g, ""));
    if (code === null || code === undefined || code > 0xffff) {
      setError(t("alphabet.invalid"));
      return;
    }
    try {
      const path = await invoke<string>("save_alphabet", { projectDir, command: code, letters });
      setSaved(
        letters.trim()
          ? t("alphabet.saved", { path })
          : t("alphabet.removed", { command: hex4(code) }),
      );
      await reload();
      await onSaved();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="panel">
      <h3>{t("alphabet.title")}</h3>
      <p className="hint left">{t("alphabet.hint")}</p>
      {alphabets.length > 0 && (
        <ul className="glossary-list">
          {alphabets.map((a) => (
            <li key={a.command}>
              <code>[{hex4(a.command)}]</code> {t("alphabet.item", { n: [...a.letters].length })}
              <button
                className="linkish"
                onClick={() => {
                  setCommand(hex4(a.command));
                  setLetters(a.letters);
                }}
              >
                {t("alphabet.edit")}
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="scan-controls">
        <label>
          {t("alphabet.command")}
          <input
            type="text"
            size={8}
            className="mono"
            value={command}
            placeholder="FF0B"
            onChange={(e) => setCommand(e.target.value)}
          />
        </label>
      </div>
      <label className="stacked">
        {t("alphabet.letters")}
        <textarea rows={3} value={letters} onChange={(e) => setLetters(e.target.value)} />
      </label>
      <div className="actions">
        <button className="primary" onClick={save} disabled={disabled || !command.trim()}>
          {t("alphabet.save")}
        </button>
      </div>
      {error && (
        <div className="error" role="alert">
          <strong>{t("common.error")}:</strong> {error}
        </div>
      )}
      {saved && <div className="ok">{saved}</div>}
    </div>
  );
}
