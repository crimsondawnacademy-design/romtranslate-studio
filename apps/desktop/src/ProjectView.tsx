import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { makeT, MessageId } from "./i18n";
import { defaultProjectDir, encodedByteLength, formatOffset, pointerCount } from "./util";
import {
  AppSettings,
  GameProject,
  GlossaryTerm,
  PLATFORM_NAMES,
  ProgressEvent,
  ScanEncoding,
  ScanOutcome,
  SettingsReport,
  PatchExportOutcome,
  ReinsertOutcome,
  TextEntry,
  TranslateSummary,
  ValidationIssue,
  ValidationReport,
} from "./types";

type StatusFilter =
  | "all"
  | "untranslated"
  | "machine"
  | "reviewed"
  | "error"
  | "too_long"
  | "placeholder_mismatch";

const STATUS_FILTERS: StatusFilter[] = [
  "all",
  "untranslated",
  "machine",
  "reviewed",
  "error",
  "too_long",
  "placeholder_mismatch",
];

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

  const [reinserting, setReinserting] = useState(false);
  const [reinsertOutcome, setReinsertOutcome] = useState<ReinsertOutcome | null>(null);
  const [allowErrors, setAllowErrors] = useState(false);
  const [patching, setPatching] = useState(false);
  const [patchOutcome, setPatchOutcome] = useState<PatchExportOutcome | null>(null);
  const [patchFormat, setPatchFormat] = useState<"auto" | "ips" | "bps">("auto");

  const [issuesByEntry, setIssuesByEntry] = useState<
    Record<string, ValidationIssue[]>
  >({});
  const [valReport, setValReport] = useState<ValidationReport | null>(null);
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

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

  async function runStructuredExtract() {
    setError(null);
    setBusy(true);
    try {
      const n = await invoke<number>("extract_structured", { projectDir });
      setScanInfo({ found: n, truncated: false });
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function runReinsert() {
    setError(null);
    setReinsertOutcome(null);
    setReinserting(true);
    try {
      const outcome = await invoke<ReinsertOutcome>("reinsert_project", {
        projectDir,
        allowErrors,
      });
      setReinsertOutcome(outcome);
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    } finally {
      setReinserting(false);
    }
  }

  async function runPatchExport() {
    setError(null);
    setPatchOutcome(null);
    setPatching(true);
    try {
      const outcome = await invoke<PatchExportOutcome>("export_patch", {
        projectDir,
        format: patchFormat === "auto" ? null : patchFormat,
      });
      setPatchOutcome(outcome);
    } catch (e) {
      setError(String(e));
    } finally {
      setPatching(false);
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

  const runValidation = useCallback(async () => {
    try {
      const report = await invoke<ValidationReport>("validate_project", { projectDir });
      const map: Record<string, ValidationIssue[]> = {};
      for (const issue of report.issues) {
        (map[issue.entryId] ??= []).push(issue);
      }
      setIssuesByEntry(map);
      setValReport(report);
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    }
  }, [projectDir, reloadEntries]);

  async function runTranslation() {
    setError(null);
    setSummary(null);
    setProgress(null);
    setTranslating(true);
    try {
      const result = await invoke<TranslateSummary>("translate_project", { projectDir });
      setSummary(result);
      await runValidation(); // erros aparecem antes de qualquer reinsercao
    } catch (e) {
      setError(String(e));
    } finally {
      setTranslating(false);
      setProgress(null);
    }
  }

  async function saveDraft() {
    if (!selectedId) return;
    setError(null);
    try {
      const issues = await invoke<ValidationIssue[]>("update_entry", {
        projectDir,
        id: selectedId,
        translation: draft,
      });
      setIssuesByEntry((prev) => ({ ...prev, [selectedId]: issues }));
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    }
  }

  async function toggleReviewed(entry: TextEntry) {
    setError(null);
    try {
      await invoke<string>("set_entry_reviewed", {
        projectDir,
        id: entry.id,
        reviewed: entry.status !== "reviewed",
      });
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    }
  }

  function selectEntry(entry: TextEntry) {
    setSelectedId(entry.id);
    setDraft(entry.translatedText ?? "");
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

  function matchesStatusFilter(e: TextEntry): boolean {
    switch (statusFilter) {
      case "all":
        return true;
      case "too_long":
        return (issuesByEntry[e.id] ?? []).some((i) => i.kind === "byte_overflow");
      case "placeholder_mismatch":
        return (issuesByEntry[e.id] ?? []).some(
          (i) => i.kind === "placeholder_mismatch",
        );
      default:
        return e.status === statusFilter;
    }
  }

  const filtered = entries.filter(
    (e) =>
      matchesStatusFilter(e) &&
      (!filter ||
        e.sourceText.toLowerCase().includes(filter.toLowerCase()) ||
        (e.translatedText ?? "").toLowerCase().includes(filter.toLowerCase())),
  );
  const shown = filtered.slice(0, RENDER_CAP);
  const selected = selectedId ? entries.find((e) => e.id === selectedId) : undefined;
  const hasResources = entries.some((e) => e.resourcePath);
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
          <button onClick={runStructuredExtract} disabled={busy || sourceBroken}>
            {t("extract.structured")}
          </button>
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
            <select
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.target.value as StatusFilter)}
            >
              {STATUS_FILTERS.map((f) => (
                <option key={f} value={f}>
                  {t(`filter.${f}` as MessageId)}
                </option>
              ))}
            </select>
            <input
              type="search"
              placeholder={t("extract.filter")}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
            <button onClick={runValidation}>{t("validate.run")}</button>
            <button onClick={() => exportAs("json")}>{t("export.json")}</button>
            <button onClick={() => exportAs("csv")}>{t("export.csv")}</button>
          </div>
          {valReport && (
            <div className={valReport.errors > 0 ? "warn" : "ok"}>
              {t("validate.summary", {
                errors: valReport.errors,
                warnings: valReport.warnings,
                checked: valReport.checked,
              })}
            </div>
          )}
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
                  {hasResources && <th>{t("table.resource")}</th>}
                  <th>{t("extract.text")}</th>
                  <th>{t("table.translation")}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {shown.map((e) => {
                  const issues = issuesByEntry[e.id] ?? [];
                  const worst = issues.some((i) => i.severity === "error")
                    ? "error"
                    : issues.length > 0
                      ? "warning"
                      : "";
                  return (
                    <tr
                      key={e.id}
                      className={`row ${worst} ${selectedId === e.id ? "selected" : ""}`}
                      onClick={() => selectEntry(e)}
                    >
                      <td className="mono">{formatOffset(e.offset)}</td>
                      {hasResources && (
                        <td className="mono resource">{e.resourcePath ?? ""}</td>
                      )}
                      <td className="text-cell">{e.sourceText}</td>
                      <td className="text-cell">{e.translatedText ?? ""}</td>
                      <td>
                        <span className={`status ${e.status}`}>
                          {t(`status.${e.status}` as MessageId)}
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          {selected ? (
            <EntryEditor
              entry={selected}
              draft={draft}
              issues={issuesByEntry[selected.id] ?? []}
              t={t}
              onDraft={setDraft}
              onSave={saveDraft}
              onToggleReviewed={() => toggleReviewed(selected)}
            />
          ) : (
            <p className="hint left">{t("editor.pick")}</p>
          )}
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

      {entries.length > 0 && (
        <div className="panel">
          <h3>{t("reinsert.title")}</h3>
          <p className="hint left">{t("reinsert.hint")}</p>
          <label className="checkline" title={t("reinsert.advancedHint")}>
            <input
              type="checkbox"
              checked={allowErrors}
              onChange={(e) => setAllowErrors(e.target.checked)}
            />
            {t("reinsert.advanced")}
          </label>
          {allowErrors && <div className="warn">{t("reinsert.advancedHint")}</div>}
          <div className="actions">
            <select
              value={patchFormat}
              onChange={(e) => setPatchFormat(e.target.value as typeof patchFormat)}
              title={t("patch.formatHint")}
            >
              <option value="auto">{t("patch.formatAuto")}</option>
              <option value="ips">IPS</option>
              <option value="bps">BPS</option>
            </select>
            <button
              onClick={runPatchExport}
              disabled={patching || reinserting || sourceBroken}
              title={t("patch.hint")}
            >
              {patching ? t("patch.exporting") : t("patch.export")}
            </button>
            <button
              className="primary"
              onClick={runReinsert}
              disabled={reinserting || sourceBroken || translating}
            >
              {reinserting ? t("reinsert.running") : t("reinsert.run")}
            </button>
          </div>
          {reinsertOutcome && (
            <>
              <div className="ok">
                {t("reinsert.done", { path: reinsertOutcome.workingPath })}
                <br />
                {t("reinsert.applied", {
                  applied: reinsertOutcome.apply.applied,
                  kept: reinsertOutcome.apply.keptOriginal,
                })}
                {reinsertOutcome.apply.relocated > 0 && (
                  <>
                    <br />
                    {t("reinsert.relocated", { n: reinsertOutcome.apply.relocated })}
                  </>
                )}
              </div>
              <ul className="evidence">
                {reinsertOutcome.verification.checks.map((c) => (
                  <li key={c}>{c}</li>
                ))}
              </ul>
              {reinsertOutcome.forcedErrors > 0 && (
                <div className="warn">
                  {t("reinsert.forced", { n: reinsertOutcome.forcedErrors })}
                </div>
              )}
            </>
          )}
          {patchOutcome && (
            <div className="ok">
              {t("patch.done", { size: patchOutcome.patchSize, format: patchOutcome.patchFormat })}
              <br />
              <span className="path">{patchOutcome.patchPath}</span>
              <span className="path">{patchOutcome.manifestPath}</span>
              <span className="path">{patchOutcome.csvPath}</span>
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

interface EntryEditorProps {
  entry: TextEntry;
  draft: string;
  issues: ValidationIssue[];
  t: ReturnType<typeof makeT>;
  onDraft: (v: string) => void;
  onSave: () => void;
  onToggleReviewed: () => void;
}

function EntryEditor({
  entry,
  draft,
  issues,
  t,
  onDraft,
  onSave,
  onToggleReviewed,
}: EntryEditorProps) {
  const draftBytes = encodedByteLength(draft, entry.encoding);
  const originalBytes = entry.originalBytes.length / 2;
  const relocatable = pointerCount(entry) > 0;
  // Realocavel so tem teto quando o adapter declara (PS1: sobra do setor).
  const limit = entry.maxBytes ?? (relocatable ? null : originalBytes);
  const overflow = draftBytes !== null && limit !== null && draftBytes > limit;
  const dirty = draft !== (entry.translatedText ?? "");

  return (
    <div className="panel editor">
      <div className="editor-cols">
        <div>
          <h4>{t("editor.original")}</h4>
          <div className="editor-original">{entry.sourceText}</div>
          <p className="hint left">
            {t("editor.bytes", { n: originalBytes })} · {formatOffset(entry.offset)}
          </p>
        </div>
        <div>
          <h4>{t("editor.translation")}</h4>
          <textarea
            rows={3}
            value={draft}
            onChange={(e) => onDraft(e.target.value)}
          />
          <p className={`hint left ${overflow ? "overflow" : ""}`}>
            {draftBytes === null
              ? t("editor.noEncoder")
              : `${t("editor.bytes", { n: draftBytes })} · ${
                  entry.maxBytes !== null
                    ? t("editor.bytesLimit", { max: entry.maxBytes })
                    : t("editor.bytesOriginal", { n: originalBytes })
                }${relocatable ? ` · ${t("editor.relocatable")}` : ""}`}
          </p>
        </div>
      </div>

      {issues.length > 0 && (
        <ul className="issues">
          {issues.map((issue, i) => (
            <li key={i} className={issue.severity}>
              {issue.message}
            </li>
          ))}
        </ul>
      )}

      <div className="actions">
        <button onClick={onToggleReviewed} disabled={!entry.translatedText}>
          {entry.status === "reviewed"
            ? t("editor.unmarkReviewed")
            : t("editor.markReviewed")}
        </button>
        <button className="primary" onClick={onSave} disabled={!dirty}>
          {t("editor.save")}
        </button>
      </div>
    </div>
  );
}
