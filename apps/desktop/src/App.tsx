import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { detectLocale, makeT, MessageId } from "./i18n";
import { defaultProjectDir, formatBytes, formatOffset } from "./util";
import {
  GameProject,
  InspectionReport,
  OpenProjectReport,
  PLATFORM_NAMES,
  ScanEncoding,
  ScanOutcome,
  SupportLevel,
} from "./types";
import "./App.css";

const TARGET_LANGUAGES = ["pt-BR", "en-US", "es-ES", "fr-FR", "de-DE", "it-IT", "ja-JP"];
const SOURCE_LANGUAGES = ["en-US", "ja-JP", "es-ES", "fr-FR", "de-DE"];
const SCAN_ENCODINGS: ScanEncoding[] = ["ascii", "utf8", "utf16_le", "utf16_be", "table"];
const RENDER_CAP = 500;

type T = ReturnType<typeof makeT>;

type Screen =
  | { kind: "home" }
  | { kind: "inspecting" }
  | { kind: "report"; report: InspectionReport }
  | { kind: "created"; project: GameProject }
  | { kind: "extract"; project: GameProject; warning: MessageId | null };

export default function App() {
  const locale = useMemo(detectLocale, []);
  const t = useMemo(() => makeT(locale), [locale]);

  const [screen, setScreen] = useState<Screen>({ kind: "home" });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [targetLanguage, setTargetLanguage] = useState(
    locale === "pt-BR" ? "pt-BR" : "en-US",
  );
  const [sourceLanguage, setSourceLanguage] = useState("");

  async function pickFile() {
    setError(null);
    const path = await open({ multiple: false, directory: false });
    if (typeof path !== "string") return;
    setScreen({ kind: "inspecting" });
    try {
      const report = await invoke<InspectionReport>("inspect_file", { path });
      setScreen({ kind: "report", report });
    } catch (e) {
      setError(String(e));
      setScreen({ kind: "home" });
    }
  }

  async function openProject() {
    setError(null);
    const dir = await open({ multiple: false, directory: true });
    if (typeof dir !== "string") return;
    try {
      const report = await invoke<OpenProjectReport>("open_project", { dir });
      const warning: MessageId | null = !report.sourceFound
        ? "project.sourceMissing"
        : report.sourceChanged
          ? "project.sourceChanged"
          : null;
      setScreen({ kind: "extract", project: report.project, warning });
    } catch (e) {
      setError(String(e));
    }
  }

  async function createProject(report: InspectionReport) {
    if (!report.best) return;
    setError(null);
    setBusy(true);
    try {
      const project = await invoke<GameProject>("create_project", {
        args: {
          sourcePath: report.path,
          platform: report.best.platform,
          adapterId: report.best.adapterId,
          sourceLanguage: sourceLanguage || null,
          targetLanguage,
          projectDir: defaultProjectDir(report.path),
        },
      });
      setScreen({ kind: "created", project });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function supportLabel(level: SupportLevel) {
    return t(`support.${level}` as MessageId);
  }

  return (
    <main className="shell">
      <header className="masthead">
        <h1>{t("app.title")}</h1>
        <p>{t("app.tagline")}</p>
      </header>

      {error && (
        <div className="error" role="alert">
          <strong>{t("common.error")}:</strong> {error}
        </div>
      )}

      {screen.kind === "home" && (
        <section className="card center">
          <div className="actions center-actions">
            <button className="primary" onClick={pickFile}>
              {t("home.selectFile")}
            </button>
            <button onClick={openProject}>{t("home.openProject")}</button>
          </div>
          <p className="hint">{t("home.hint")}</p>
        </section>
      )}

      {screen.kind === "inspecting" && (
        <section className="card center">
          <p>{t("inspect.loading")}</p>
        </section>
      )}

      {screen.kind === "report" && (
        <ReportView
          report={screen.report}
          t={t}
          busy={busy}
          targetLanguage={targetLanguage}
          sourceLanguage={sourceLanguage}
          onTargetLanguage={setTargetLanguage}
          onSourceLanguage={setSourceLanguage}
          onCreate={() => createProject(screen.report)}
          onBack={() => setScreen({ kind: "home" })}
          supportLabel={supportLabel}
        />
      )}

      {screen.kind === "created" && (
        <section className="card">
          <h2>{t("project.createdTitle")}</h2>
          <p>{t("project.createdAt")}</p>
          <code className="path">{defaultProjectDir(screen.project.sourcePath)}</code>
          <div className="actions">
            <button onClick={() => setScreen({ kind: "home" })}>
              {t("common.back")}
            </button>
            <button
              className="primary"
              onClick={() =>
                setScreen({ kind: "extract", project: screen.project, warning: null })
              }
            >
              {t("project.extract")}
            </button>
          </div>
        </section>
      )}

      {screen.kind === "extract" && (
        <ExtractView
          project={screen.project}
          warning={screen.warning}
          t={t}
          onBack={() => setScreen({ kind: "home" })}
        />
      )}
    </main>
  );
}

interface ReportViewProps {
  report: InspectionReport;
  t: T;
  busy: boolean;
  targetLanguage: string;
  sourceLanguage: string;
  onTargetLanguage: (v: string) => void;
  onSourceLanguage: (v: string) => void;
  onCreate: () => void;
  onBack: () => void;
  supportLabel: (level: SupportLevel) => string;
}

function ReportView({
  report,
  t,
  busy,
  targetLanguage,
  sourceLanguage,
  onTargetLanguage,
  onSourceLanguage,
  onCreate,
  onBack,
  supportLabel,
}: ReportViewProps) {
  const best = report.best;
  const others = report.results.filter((r) => r.adapterId !== best?.adapterId);

  return (
    <section className="card">
      <h2>{t("inspect.title")}</h2>

      <dl className="facts">
        <dt>{t("inspect.file")}</dt>
        <dd className="path">{report.path}</dd>
        <dt>{t("inspect.size")}</dt>
        <dd>{formatBytes(report.size)}</dd>
        <dt>{t("inspect.sha256")}</dt>
        <dd className="path">{report.sha256}</dd>
      </dl>

      {best ? (
        <>
          <div className="verdict">
            <div className="verdict-main">
              <span className="platform">{PLATFORM_NAMES[best.platform]}</span>
              <span className={`badge ${best.supportLevel}`}>
                {supportLabel(best.supportLevel)}
              </span>
            </div>
            <div className="verdict-meta">
              {t("inspect.adapter")}: <code>{best.adapterId}</code> ·{" "}
              {t("inspect.confidence")}: {(best.confidence * 100).toFixed(0)}%
            </div>
            <ul className="evidence">
              {best.evidence.map((e) => (
                <li key={e}>{e}</li>
              ))}
            </ul>
          </div>

          <div className="languages">
            <label>
              {t("project.sourceLanguage")}
              <select
                value={sourceLanguage}
                onChange={(e) => onSourceLanguage(e.target.value)}
              >
                <option value="">{t("project.auto")}</option>
                {SOURCE_LANGUAGES.map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
            <label>
              {t("project.targetLanguage")}
              <select
                value={targetLanguage}
                onChange={(e) => onTargetLanguage(e.target.value)}
              >
                {TARGET_LANGUAGES.map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="actions">
            <button onClick={onBack}>{t("common.back")}</button>
            <button className="primary" onClick={onCreate} disabled={busy}>
              {busy ? t("project.creating") : t("project.create")}
            </button>
          </div>
        </>
      ) : (
        <>
          <p className="no-match">{t("inspect.noMatch")}</p>
          <div className="actions">
            <button onClick={onBack}>{t("common.back")}</button>
          </div>
        </>
      )}

      {others.length > 0 && (
        <details>
          <summary>{t("inspect.otherCandidates")}</summary>
          <ul className="evidence">
            {others.map((r) => (
              <li key={r.adapterId}>
                {PLATFORM_NAMES[r.platform]} — <code>{r.adapterId}</code> (
                {(r.confidence * 100).toFixed(0)}%)
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

interface ExtractViewProps {
  project: GameProject;
  warning: MessageId | null;
  t: T;
  onBack: () => void;
}

function ExtractView({ project, warning, t, onBack }: ExtractViewProps) {
  const [encoding, setEncoding] = useState<ScanEncoding>("ascii");
  const [minChars, setMinChars] = useState(4);
  const [tblPath, setTblPath] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ScanOutcome | null>(null);
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const sourceBroken = warning === "project.sourceMissing";

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
      const result = await invoke<ScanOutcome>("scan_file", {
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
      setOutcome(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function exportAs(format: "json" | "csv") {
    if (!outcome) return;
    setError(null);
    const defaultPath = `${defaultProjectDir(project.sourcePath)}/extracted/strings.${format}`;
    const path = await save({
      defaultPath,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (typeof path !== "string") return;
    try {
      const saved = await invoke<string>("export_entries", {
        entries: outcome.entries,
        path,
        format,
      });
      setSavedPath(saved);
    } catch (e) {
      setError(String(e));
    }
  }

  const filtered = outcome
    ? filter
      ? outcome.entries.filter((e) =>
          e.sourceText.toLowerCase().includes(filter.toLowerCase()),
        )
      : outcome.entries
    : [];
  const shown = filtered.slice(0, RENDER_CAP);

  return (
    <section className="card">
      <h2>{t("extract.title")}</h2>
      <dl className="facts">
        <dt>{t("inspect.file")}</dt>
        <dd className="path">{project.sourcePath}</dd>
        <dt>{t("inspect.platform")}</dt>
        <dd>{PLATFORM_NAMES[project.platform]}</dd>
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

      {outcome && (
        <>
          <div className="results-bar">
            <span>{t("extract.found", { n: outcome.entries.length })}</span>
            <input
              type="search"
              placeholder={t("extract.filter")}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
            <button onClick={() => exportAs("json")} disabled={!outcome.entries.length}>
              {t("export.json")}
            </button>
            <button onClick={() => exportAs("csv")} disabled={!outcome.entries.length}>
              {t("export.csv")}
            </button>
          </div>
          {outcome.truncated && (
            <div className="warn">{t("extract.truncated", { n: 20000 })}</div>
          )}
          {savedPath && (
            <div className="ok">{t("export.saved", { path: savedPath })}</div>
          )}

          {outcome.entries.length === 0 ? (
            <p className="no-match">{t("extract.none")}</p>
          ) : (
            <>
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
                      <th>{t("extract.bytes")}</th>
                      <th>{t("extract.text")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {shown.map((e) => (
                      <tr key={e.id}>
                        <td className="mono">{formatOffset(e.offset)}</td>
                        <td className="mono num">{e.originalBytes.length / 2}</td>
                        <td className="text-cell">{e.sourceText}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </>
      )}
    </section>
  );
}
