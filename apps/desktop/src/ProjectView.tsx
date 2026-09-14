import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { makeT, MessageId } from "./i18n";
import { defaultProjectDir, formatOffset } from "./util";
import {
  AppSettings,
  GameProject,
  GlossaryTerm,
  PLATFORM_NAMES,
  ProgressEvent,
  ScanEncoding,
  ScanOutcome,
  SettingsReport,
  TextEntry,
  TranslateSummary,
} from "./types";

const SCAN_ENCODINGS: ScanEncoding[] = ["ascii", "utf8", "utf16_le", "utf16_be", "table"];
const RENDER_CAP = 500;

interface ProjectViewProps {
  project: GameProject;
  projectDir: string;
  warning: MessageId | null;
  t: ReturnType<typeof makeT>;
  onBack: () => void;
}

export default function ProjectView({
  project,
  projectDir,
  warning,
  t,
  onBack,
}: ProjectViewProps) {
  const [entries, setEntries] = useState<TextEntry[]>([]);
  const [scanInfo, setScanInfo] = useState<{ found: number; truncated: boolean } | null>(
    null,
  );
  const [encoding, setEncoding] = useState<ScanEncoding>("ascii");
  const [minChars, setMinChars] = useState(4);
  const [tblPath, setTblPath] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [apiKeySet, setApiKeySet] = useState(false);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [testResult, setTestResult] = useState<string | null>(null);

  const [translating, setTranslating] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [summary, setSummary] = useState<TranslateSummary | null>(null);

  const [glossary, setGlossary] = useState<GlossaryTerm[]>([]);
  const [gTerm, setGTerm] = useState("");
  const [gTranslation, setGTranslation] = useState("");
  const [gNoTranslate, setGNoTranslate] = useState(false);

  const sourceBroken = warning === "project.sourceMissing";

  const reloadEntries = useCallback(async () => {
    try {
      setEntries(await invoke<TextEntry[]>("load_entries", { projectDir }));
    } catch (e) {
      setError(String(e));
    }
  }, [projectDir]);

  const reloadGlossary = useCallback(async () => {
    try {
      setGlossary(await invoke<GlossaryTerm[]>("glossary_list", { projectDir }));
    } catch (e) {
      setError(String(e));
    }
  }, [projectDir]);

  useEffect(() => {
    void reloadEntries();
    void reloadGlossary();
    invoke<SettingsReport>("get_settings")
      .then((r) => {
        setSettings(r.settings);
        setApiKeySet(r.apiKeySet);
      })
      .catch((e) => setError(String(e)));
    const unlisten = listen<ProgressEvent>("translation-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [reloadEntries, reloadGlossary]);

  async function chooseTbl() {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Table", extensions: ["tbl", "txt"] }],
    });
    if (typeof path === "string") setTblPath(path);
  }

  async function runScan() {
    if (encoding === "table" && !tblPath) {
      setError(t("extract.tblMissing"));
      return;
    }
    setError(null);
    setSavedPath(null);
    setBusy(true);
    try {
      const outcome = await invoke<ScanOutcome>("scan_file", {
        path: project.sourcePath,
        config: {
          encoding,
          tblPath: encoding === "table" ? tblPath : null,
          minChars,
          regionStart: null,
          regionEnd: null,
          maxEntries: 20000,
        },
      });
      await invoke<number>("save_entries", { projectDir, entries: outcome.entries });
      setScanInfo({ found: outcome.entries.length, truncated: outcome.truncated });
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function exportAs(format: "json" | "csv") {
    if (!entries.length) return;
    setError(null);
    const defaultPath = `${defaultProjectDir(project.sourcePath)}/extracted/strings.${format}`;
    const path = await save({
      defaultPath,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (typeof path !== "string") return;
    try {
      const saved = await invoke<string>("export_entries", { entries, path, format });
      setSavedPath(saved);
    } catch (e) {
      setError(String(e));
    }
  }

  async function saveSettings() {
    if (!settings) return;
    setError(null);
    setTestResult(null);
    try {
      const report = await invoke<SettingsReport>("save_settings", {
        newSettings: settings,
        apiKey: apiKeyInput ? apiKeyInput : null,
      });
      setSettings(report.settings);
      setApiKeySet(report.apiKeySet);
      setApiKeyInput("");
    } catch (e) {
      setError(String(e));
    }
  }

  async function testProvider() {
    setError(null);
    setTestResult(null);
    try {
      const model = await invoke<string>("test_provider");
      setTestResult(t("translate.testOk", { model }));
    } catch (e) {
      setError(String(e));
    }
  }

  async function runTranslation() {
    setError(null);
    setSummary(null);
    setProgress(null);
    setTranslating(true);
    try {
      const result = await invoke<TranslateSummary>("translate_project", { projectDir });
      setSummary(result);
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    } finally {
      setTranslating(false);
      setProgress(null);
    }
  }

  async function cancelTranslation() {
    try {
      await invoke("cancel_translation");
    } catch (e) {
      setError(String(e));
    }
  }

  async function addGlossaryTerm() {
    if (!gTerm.trim()) return;
    setError(null);
    try {
      await invoke("glossary_upsert", {
        projectDir,
        term: {
          term: gTerm.trim(),
          translation: gNoTranslate ? null : gTranslation.trim() || null,
          noTranslate: gNoTranslate,
          caseSensitive: false,
          note: null,
        },
      });
      setGTerm("");
      setGTranslation("");
      setGNoTranslate(false);
      await reloadGlossary();
    } catch (e) {
      setError(String(e));
    }
  }

  async function removeGlossaryTerm(term: string) {
    try {
      await invoke("glossary_delete", { projectDir, term });
      await reloadGlossary();
    } catch (e) {
      setError(String(e));
    }
  }

  function updateEndpoint(
    key: "ollama" | "openaiCompatible",
    field: "baseUrl" | "model",
    value: string,
  ) {
    if (!settings) return;
    setSettings({ ...settings, [key]: { ...settings[key], [field]: value } });
  }

  const filtered = filter
    ? entries.filter(
        (e) =>
          e.sourceText.toLowerCase().includes(filter.toLowerCase()) ||
          (e.translatedText ?? "").toLowerCase().includes(filter.toLowerCase()),
      )
    : entries;
  const shown = filtered.slice(0, RENDER_CAP);
  const endpoint = settings
    ? settings.provider === "ollama"
      ? settings.ollama
      : settings.openaiCompatible
    : null;
  const endpointKey = settings?.provider === "ollama" ? "ollama" : "openaiCompatible";
  const pending = entries.filter((e) => !e.translatedText).length;

  return (
    <section className="card">
      <h2>{t("extract.title")}</h2>
      <dl className="facts">
        <dt>{t("inspect.file")}</dt>
        <dd className="path">{project.sourcePath}</dd>
        <dt>{t("inspect.platform")}</dt>
        <dd>
          {PLATFORM_NAMES[project.platform]} · {project.targetLanguage}
        </dd>
      </dl>

      {warning && (
        <div className={sourceBroken ? "error" : "warn"} role="alert">
          {t(warning)}
        </div>
      )}
      {error && (
        <div className="error" role="alert">
          <strong>{t("common.error")}:</strong> {error}
        </div>
      )}

      <div className="scan-controls">
        <label>
          {t("extract.encoding")}
          <select
            value={encoding}
            onChange={(e) => setEncoding(e.target.value as ScanEncoding)}
          >
            {SCAN_ENCODINGS.map((enc) => (
              <option key={enc} value={enc}>
                {enc}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("extract.minChars")}
          <input
            type="number"
            min={1}
            max={64}
            value={minChars}
            onChange={(e) => setMinChars(Number(e.target.value) || 1)}
          />
        </label>
        {encoding === "table" && (
          <label>
            {t("extract.chooseTbl")}
            <button onClick={chooseTbl}>
              {tblPath ? tblPath.split(/[\\/]/).pop() : t("extract.chooseTbl")}
            </button>
          </label>
        )}
        <div className="scan-run">
          <button onClick={onBack}>{t("common.back")}</button>
          <button className="primary" onClick={runScan} disabled={busy || sourceBroken}>
            {busy ? t("extract.scanning") : t("extract.scan")}
          </button>
        </div>
      </div>
      <p className="hint left">{t("extract.hint")}</p>

      {scanInfo && (
        <div className="ok">
          {t("entries.saved", { n: scanInfo.found })}
          {scanInfo.truncated && ` — ${t("extract.truncated", { n: 20000 })}`}
        </div>
      )}
      {savedPath && <div className="ok">{t("export.saved", { path: savedPath })}</div>}

      {entries.length > 0 && (
        <>
          <div className="results-bar">
            <span>{t("extract.found", { n: entries.length })}</span>
            <input
              type="search"
              placeholder={t("extract.filter")}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
            <button onClick={() => exportAs("json")}>{t("export.json")}</button>
            <button onClick={() => exportAs("csv")}>{t("export.csv")}</button>
          </div>
          {filtered.length > RENDER_CAP && (
            <p className="hint left">
              {t("extract.showing", { shown: RENDER_CAP, total: filtered.length })}
            </p>
          )}
          <div className="strings-wrap">
            <table className="strings">
              <thead>
                <tr>
                  <th>{t("extract.offset")}</th>
                  <th>{t("extract.text")}</th>
                  <th>{t("table.translation")}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {shown.map((e) => (
                  <tr key={e.id}>
                    <td className="mono">{formatOffset(e.offset)}</td>
                    <td className="text-cell">{e.sourceText}</td>
                    <td className="text-cell">{e.translatedText ?? ""}</td>
                    <td>
                      <span className={`status ${e.status}`}>
                        {t(`status.${e.status}` as MessageId)}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}

      {settings && endpoint && (
        <div className="panel">
          <h3>{t("translate.title")}</h3>
          <div className="scan-controls">
            <label>
              {t("translate.provider")}
              <select
                value={settings.provider}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    provider: e.target.value as AppSettings["provider"],
                  })
                }
              >
                <option value="ollama">Ollama (local)</option>
                <option value="openai_compatible">OpenAI-compatible</option>
              </select>
            </label>
            <label>
              {t("translate.baseUrl")}
              <input
                type="text"
                size={28}
                value={endpoint.baseUrl}
                onChange={(e) => updateEndpoint(endpointKey, "baseUrl", e.target.value)}
              />
            </label>
            <label>
              {t("translate.model")}
              <input
                type="text"
                size={18}
                value={endpoint.model}
                placeholder={settings.provider === "ollama" ? "llama3.2:3b" : "gpt-5-mini"}
                onChange={(e) => updateEndpoint(endpointKey, "model", e.target.value)}
              />
            </label>
            {settings.provider === "openai_compatible" && (
              <label>
                {t("translate.apiKey")} {apiKeySet && <em>{t("translate.apiKeySet")}</em>}
                <input
                  type="password"
                  size={22}
                  value={apiKeyInput}
                  onChange={(e) => setApiKeyInput(e.target.value)}
                />
              </label>
            )}
          </div>
          {settings.provider === "openai_compatible" && (
            <label className="checkline">
              <input
                type="checkbox"
                checked={settings.allowRemoteTranslation}
                onChange={(e) =>
                  setSettings({ ...settings, allowRemoteTranslation: e.target.checked })
                }
              />
              {t("translate.allowRemote")}
            </label>
          )}
          <div className="actions">
            <button onClick={saveSettings}>{t("translate.saveSettings")}</button>
            <button onClick={testProvider}>{t("translate.test")}</button>
            {translating ? (
              <button onClick={cancelTranslation}>{t("translate.cancel")}</button>
            ) : (
              <button
                className="primary"
                onClick={runTranslation}
                disabled={pending === 0 || sourceBroken}
              >
                {t("translate.run")} ({pending})
              </button>
            )}
          </div>
          {testResult && <div className="ok">{testResult}</div>}
          {translating && (
            <div className="progress-row">
              <progress value={progress?.done ?? 0} max={progress?.total ?? 1} />
              <span>
                {progress
                  ? `${progress.done}/${progress.total} — ${t(
                      `translate.phase.${progress.phase}` as MessageId,
                    )}`
                  : t("translate.running")}
              </span>
            </div>
          )}
          {summary && (
            <div className="ok">
              {t("translate.summary", {
                translated: summary.translated,
                tmHits: summary.tmHits,
                failed: summary.failed,
              })}
              {summary.cancelled && t("translate.summaryCancelled")}
            </div>
          )}
        </div>
      )}

      <details className="panel">
        <summary>
          {t("glossary.title")} ({glossary.length})
        </summary>
        <div className="glossary-form">
          <input
            type="text"
            placeholder={t("glossary.term")}
            value={gTerm}
            onChange={(e) => setGTerm(e.target.value)}
          />
          <input
            type="text"
            placeholder={t("glossary.translation")}
            value={gTranslation}
            disabled={gNoTranslate}
            onChange={(e) => setGTranslation(e.target.value)}
          />
          <label className="checkline">
            <input
              type="checkbox"
              checked={gNoTranslate}
              onChange={(e) => setGNoTranslate(e.target.checked)}
            />
            {t("glossary.noTranslate")}
          </label>
          <button onClick={addGlossaryTerm} disabled={!gTerm.trim()}>
            {t("glossary.add")}
          </button>
        </div>
        {glossary.length === 0 ? (
          <p className="hint left">{t("glossary.empty")}</p>
        ) : (
          <ul className="glossary-list">
            {glossary.map((g) => (
              <li key={g.term}>
                <code>{g.term}</code> →{" "}
                {g.noTranslate ? <em>{t("glossary.noTranslate")}</em> : g.translation}
                <button className="linkish" onClick={() => removeGlossaryTerm(g.term)}>
                  {t("glossary.remove")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </details>
    </section>
  );
}
