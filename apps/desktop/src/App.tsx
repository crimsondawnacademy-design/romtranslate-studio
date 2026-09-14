import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { detectLocale, makeT } from "./i18n";
import { defaultProjectDir, formatBytes } from "./util";
import {
  GameProject,
  InspectionReport,
  PLATFORM_NAMES,
  SupportLevel,
} from "./types";
import "./App.css";

const TARGET_LANGUAGES = ["pt-BR", "en-US", "es-ES", "fr-FR", "de-DE", "it-IT", "ja-JP"];
const SOURCE_LANGUAGES = ["en-US", "ja-JP", "es-ES", "fr-FR", "de-DE"];

type Screen =
  | { kind: "home" }
  | { kind: "inspecting" }
  | { kind: "report"; report: InspectionReport }
  | { kind: "created"; project: GameProject };

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
    return t(`support.${level}` as Parameters<typeof t>[0]);
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
          <button className="primary" onClick={pickFile}>
            {t("home.selectFile")}
          </button>
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
          <p className="hint">{t("project.next")}</p>
          <button onClick={() => setScreen({ kind: "home" })}>{t("common.back")}</button>
        </section>
      )}
    </main>
  );
}

interface ReportViewProps {
  report: InspectionReport;
  t: ReturnType<typeof makeT>;
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
