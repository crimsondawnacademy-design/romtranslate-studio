import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { makeT, MessageId } from "./i18n";
import {
  defaultProjectDir,
  encodedByteLength,
  formatBytes,
  formatOffset,
  pointerCount,
} from "./util";
import {
  AppSettings,
  CLAUDE_PRICES,
  EstimateReport,
  ExtractOutcome,
  GameProject,
  GlossaryTerm,
  PLATFORM_NAMES,
  ProgressEvent,
  RelativeCandidate,
  ScanEncoding,
  ScanOutcome,
  SettingsReport,
  PatchExportOutcome,
  ReinsertOutcome,
  TextEntry,
  TranslateSummary,
  ValidationIssue,
  ValidationReport,
  XrayReport,
} from "./types";

type StatusFilter =
  | "all"
  | "untranslated"
  | "machine"
  | "reviewed"
  | "error"
  | "internal"
  | "too_long"
  | "placeholder_mismatch";

const STATUS_FILTERS: StatusFilter[] = [
  "all",
  "untranslated",
  "machine",
  "reviewed",
  "error",
  "internal",
  "too_long",
  "placeholder_mismatch",
];

const SCAN_ENCODINGS: ScanEncoding[] = [
  "ascii",
  "utf8",
  "utf16_le",
  "utf16_be",
  "shift_jis",
  "table",
];
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
  const [scanInfo, setScanInfo] = useState<{
    found: number;
    truncated: boolean;
    discarded: number;
    /** So a extracao estruturada tira entry antiga do projeto. */
    removed: number;
    /** Fora das regioes de texto: tiradas da traducao pela extracao estruturada. */
    stray: number;
    /** Das salvas, quantas estavam dentro de blocos comprimidos. */
    packed: number;
    /** Das salvas, quantas sao japones em codigos de 16 bits (tabela gerada). */
    jis16: number;
    structured: boolean;
  } | null>(null);
  const [encoding, setEncoding] = useState<ScanEncoding>("ascii");
  const [minChars, setMinChars] = useState(4);
  const [onlyText, setOnlyText] = useState(true);
  const [tblPath, setTblPath] = useState<string | null>(null);
  const [xray, setXray] = useState<XrayReport | null>(null);
  const [relWord, setRelWord] = useState("");
  const [relCandidates, setRelCandidates] = useState<RelativeCandidate[] | null>(null);
  // Bases ja adotadas: uma busca em maiusculas e outra em minusculas se somam.
  const [tblBases, setTblBases] = useState<{ upper: number | null; lower: number | null }>({
    upper: null,
    lower: null,
  });
  const [tblSaved, setTblSaved] = useState(false);
  const [tblAdopted, setTblAdopted] = useState(false);
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [apiKeySet, setApiKeySet] = useState(false);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [anthropicKeySet, setAnthropicKeySet] = useState(false);
  const [anthropicKeyInput, setAnthropicKeyInput] = useState("");
  const [estimate, setEstimate] = useState<EstimateReport | null>(null);
  const [estimating, setEstimating] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  // Conta do ChatGPT em uso pelo Codex: null = ainda nao conferido.
  const [codexLogged, setCodexLogged] = useState<boolean | null>(null);
  const [loggingIn, setLoggingIn] = useState(false);
  // O backend recusou por falta da permissao de traducao remota: em vez de
  // um erro sem saida, a tela pergunta e segue com a acao que foi pedida.
  const [remoteAsk, setRemoteAsk] = useState<"test" | "translate" | null>(null);
  const [reinsertError, setReinsertError] = useState<string | null>(null);
  // Erro de configurar, testar ou traduzir aparece junto dos botoes: o aviso
  // geral fica no topo da pagina, fora de vista de quem esta neste painel.
  const [translateError, setTranslateError] = useState<string | null>(null);

  const [translating, setTranslating] = useState(false);
  // "Cancelar" foi clicado e a rodada ainda nao devolveu o resumo.
  const [cancelling, setCancelling] = useState(false);
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
        setAnthropicKeySet(r.anthropicKeySet);
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
    if (typeof path === "string") {
      setTblPath(path);
      setTblSaved(false);
      setTblAdopted(false);
    }
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
      const outcome = await invoke<ScanOutcome>("scan_project", {
        projectDir,
        config: {
          encoding,
          tblPath: encoding === "table" ? tblPath : null,
          minChars,
          regionStart: null,
          regionEnd: null,
          onlyText,
        },
      });
      await invoke<number>("save_entries", { projectDir, entries: outcome.entries });
      // Tabela de fora foi copiada pro projeto: e a copia que vale agora.
      if (outcome.tblPath && outcome.tblPath !== tblPath) {
        setTblPath(outcome.tblPath);
        setTblAdopted(true);
      }
      setScanInfo({
        found: outcome.entries.length,
        truncated: outcome.truncated,
        discarded: outcome.discarded,
        removed: 0,
        stray: 0,
        packed: 0,
        jis16: 0,
        structured: false,
      });
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function runXray() {
    setError(null);
    setBusy(true);
    try {
      setXray(await invoke<XrayReport>("xray_project", { projectDir }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function runRelativeSearch() {
    setError(null);
    setBusy(true);
    try {
      setRelCandidates(
        await invoke<RelativeCandidate[]>("relative_search", {
          projectDir,
          word: relWord.trim(),
        }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function adoptCandidate(candidate: RelativeCandidate) {
    const bases = {
      upper: candidate.upperBase ?? tblBases.upper,
      lower: candidate.lowerBase ?? tblBases.lower,
    };
    setError(null);
    try {
      const path = await invoke<string>("save_relative_tbl", {
        projectDir,
        upperBase: bases.upper,
        lowerBase: bases.lower,
      });
      setTblBases(bases);
      setTblPath(path);
      setTblSaved(true);
      setTblAdopted(false);
    } catch (e) {
      setError(String(e));
    }
  }

  async function runStructuredExtract() {
    setError(null);
    setBusy(true);
    try {
      const outcome = await invoke<ExtractOutcome>("extract_structured", { projectDir });
      setScanInfo({
        found: outcome.saved,
        truncated: outcome.truncated,
        discarded: outcome.discarded,
        removed: outcome.removed,
        stray: outcome.stray,
        packed: outcome.packed,
        jis16: outcome.jis16,
        structured: true,
      });
      await reloadEntries();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function runReinsert() {
    setReinsertError(null);
    setReinsertOutcome(null);
    setReinserting(true);
    try {
      // Padrao: a traducao com erro fica de fora e a frase segue no original.
      const outcome = await invoke<ReinsertOutcome>("reinsert_project", {
        projectDir,
        policy: allowErrors ? "force" : "skip",
      });
      setReinsertOutcome(outcome);
      await reloadEntries();
    } catch (e) {
      setReinsertError(String(e));
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

  /** O erro e a trava de privacidade, e nao uma falha? */
  function needsRemoteConsent(e: unknown): boolean {
    return String(e).includes("traducao remota desabilitada");
  }

  /** A pessoa autorizou: marca a permissao e refaz a acao que tinha pedido. */
  async function allowRemoteAndContinue() {
    if (!settings || !remoteAsk) return;
    const action = remoteAsk;
    const allowed = { ...settings, allowRemoteTranslation: true };
    setRemoteAsk(null);
    setSettings(allowed);
    if (action === "test") await testProvider(allowed);
    else await runTranslation(allowed);
  }

  /** Grava o que esta na tela. `false` = falhou, com o erro ja exibido. */
  async function saveSettings(current: AppSettings | null = settings): Promise<boolean> {
    if (!current) return false;
    setTranslateError(null);
    setTestResult(null);
    try {
      const report = await invoke<SettingsReport>("save_settings", {
        newSettings: current,
        apiKey: apiKeyInput ? apiKeyInput : null,
        anthropicKey: anthropicKeyInput ? anthropicKeyInput : null,
      });
      setSettings(report.settings);
      setApiKeySet(report.apiKeySet);
      setApiKeyInput("");
      setAnthropicKeySet(report.anthropicKeySet);
      setAnthropicKeyInput("");
      return true;
    } catch (e) {
      setTranslateError(String(e));
      return false;
    }
  }

  const codexSelected = settings?.provider === "codex_cli";
  const codexCommand = settings?.codexCli.command ?? "";
  const codexOwnAccount = settings?.codexCli.ownAccount ?? false;

  const refreshCodexAccount = useCallback(async () => {
    setCodexLogged(null);
    try {
      setCodexLogged(
        await invoke<boolean>("codex_account_status", {
          command: codexCommand,
          ownAccount: codexOwnAccount,
        }),
      );
    } catch (e) {
      setCodexLogged(false);
      setTranslateError(String(e));
    }
  }, [codexCommand, codexOwnAccount]);

  // O comando muda a cada tecla: so reconfere quando o provider ou a conta
  // escolhida mudam (e depois de entrar ou sair).
  useEffect(() => {
    if (codexSelected) void refreshCodexAccount();
  }, [codexSelected, codexOwnAccount]);

  /** Login da conta do app: o navegador abre e a pessoa escolhe a conta la. */
  async function codexLogin(switching: boolean) {
    setTranslateError(null);
    setTestResult(null);
    setLoggingIn(true);
    try {
      if (switching) await invoke("codex_logout", { command: codexCommand });
      await invoke("codex_login", { command: codexCommand });
    } catch (e) {
      setTranslateError(String(e));
    } finally {
      setLoggingIn(false);
      await refreshCodexAccount();
    }
  }

  async function codexLogout() {
    setTranslateError(null);
    setTestResult(null);
    try {
      await invoke("codex_logout", { command: codexCommand });
    } catch (e) {
      setTranslateError(String(e));
    }
    await refreshCodexAccount();
  }

  /** Quanto a rodada de agora custaria: lotes, tokens e, com preco, dolares. */
  async function runEstimate() {
    setEstimating(true);
    setEstimate(null);
    try {
      if (!(await saveSettings())) return;
      setEstimate(await invoke<EstimateReport>("estimate_translation", { projectDir }));
    } catch (e) {
      setTranslateError(String(e));
    } finally {
      setEstimating(false);
    }
  }

  async function testProvider(current: AppSettings | null = settings) {
    setTesting(true);
    setRemoteAsk(null);
    try {
      // O backend testa a configuracao GRAVADA. Sem gravar antes, escolher um
      // provider e clicar em Testar ainda testava o anterior.
      if (!(await saveSettings(current))) return;
      const model = await invoke<string>("test_provider");
      setTestResult(t("translate.testOk", { model }));
    } catch (e) {
      if (needsRemoteConsent(e)) setRemoteAsk("test");
      else setTranslateError(String(e));
    } finally {
      setTesting(false);
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

  async function runTranslation(current: AppSettings | null = settings) {
    setSummary(null);
    setProgress(null);
    setRemoteAsk(null);
    setTranslating(true);
    try {
      // Traduz com o provider que esta na tela, nao com o ultimo gravado.
      if (!(await saveSettings(current))) return;
      const result = await invoke<TranslateSummary>("translate_project", { projectDir });
      setSummary(result);
      if (result.stopped) setTranslateError(t("translate.stopped", { error: result.stopped }));
      await runValidation(); // erros aparecem antes de qualquer reinsercao
    } catch (e) {
      if (needsRemoteConsent(e)) setRemoteAsk("translate");
      else setTranslateError(String(e));
    } finally {
      setTranslating(false);
      setCancelling(false);
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

  async function unmarkInternal(entry: TextEntry) {
    setError(null);
    try {
      await invoke("unmark_internal", { projectDir, id: entry.id });
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
    setCancelling(true);
    try {
      await invoke("cancel_translation");
    } catch (e) {
      setCancelling(false);
      setTranslateError(String(e));
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
    key: "ollama" | "openaiCompatible" | "anthropic",
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
  const endpointKey =
    settings?.provider === "ollama"
      ? "ollama"
      : settings?.provider === "anthropic"
        ? "anthropic"
        : "openaiCompatible";
  const endpoint = settings ? settings[endpointKey] : null;
  // Gasto so faz sentido em provider cobrado por token com chave da pessoa.
  const paidByToken =
    settings?.provider === "anthropic" || settings?.provider === "openai_compatible";
  // Preco informado vence; sem ele, a tabela dos modelos do Claude que a tela
  // conhece; sem nenhum dos dois, a tela mostra so os tokens.
  const prices: [number, number] | null = !settings
    ? null
    : settings.priceInput > 0 || settings.priceOutput > 0
      ? [settings.priceInput, settings.priceOutput]
      : settings.provider === "anthropic"
        ? (CLAUDE_PRICES[settings.anthropic.model.trim()] ?? null)
        : null;
  const dollars = (inputTokens: number, outputTokens: number) =>
    prices ? (inputTokens * prices[0] + outputTokens * prices[1]) / 1_000_000 : null;
  const money = (value: number) => (value < 0.01 ? value.toFixed(4) : value.toFixed(2));
  // Nome interno do jogo nao e texto: fica fora da fila, contado na tela.
  const internal = entries.filter((e) => e.status === "internal").length;
  const pending = entries.filter((e) => !e.translatedText && e.status !== "internal").length;
  const hasAscii = entries.some((e) => e.encoding === "ascii");
  const hasJapanese = entries.some((e) => e.encoding === "shift_jis");
  const hasJis16 = entries.some(
    (e) => typeof e.encoding === "object" && e.encoding.table === "jis16",
  );

  return (
    <section className="card">
      <h2 data-step="1">{t("extract.title")}</h2>
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
        <label className="checkline" title={t("extract.onlyTextHint")}>
          <input
            type="checkbox"
            checked={onlyText}
            onChange={(e) => setOnlyText(e.target.checked)}
          />
          {t("extract.onlyText")}
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
          <button onClick={runXray} disabled={busy || sourceBroken}>
            {t("extract.xray")}
          </button>
          <button onClick={runScan} disabled={busy || sourceBroken}>
            {busy ? t("extract.scanning") : t("extract.scan")}
          </button>
          <button
            className="primary"
            onClick={runStructuredExtract}
            disabled={busy || sourceBroken}
          >
            {t("extract.structured")}
          </button>
        </div>
      </div>
      <p className="hint left">{t("extract.hint")}</p>

      {encoding === "table" && (
        <div className="panel">
          <h3>{t("relative.title")}</h3>
          <p className="hint left">{t("relative.hint")}</p>
          <div className="scan-controls">
            <label>
              {t("relative.word")}
              <input
                type="text"
                size={18}
                value={relWord}
                placeholder="START"
                onChange={(e) => setRelWord(e.target.value)}
              />
            </label>
            <button
              onClick={runRelativeSearch}
              disabled={busy || sourceBroken || relWord.trim().length < 4}
            >
              {t("relative.search")}
            </button>
          </div>
          {relCandidates?.length === 0 && <p className="hint left">{t("relative.none")}</p>}
          {relCandidates && relCandidates.length > 0 && (
            <table className="strings">
              <thead>
                <tr>
                  <th>{t("relative.preview")}</th>
                  <th>{t("relative.hits")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {relCandidates.map((c) => (
                  <tr key={`${c.upperBase}-${c.lowerBase}`}>
                    <td className="mono">{c.preview}</td>
                    <td>{c.hits}</td>
                    <td>
                      {c.ascii ? (
                        t("relative.ascii")
                      ) : (
                        <button onClick={() => adoptCandidate(c)}>{t("relative.use")}</button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {tblSaved && tblPath && (
            <div className="ok">{t("relative.saved", { path: tblPath })}</div>
          )}
          {tblAdopted && tblPath && (
            <div className="ok">{t("relative.adopted", { path: tblPath })}</div>
          )}
          <p className="hint left">{t("relative.writeBack")}</p>
        </div>
      )}

      {xray && (
        <div className="panel">
          <h3>{t("xray.title")}</h3>
          <p className="hint left">{t("xray.hint")}</p>
          <ul className="issues">
            <li>
              {xray.entries > 0
                ? t("xray.relocatable", { n: xray.relocatable, total: xray.entries })
                : t("xray.noEntries")}
            </li>
            <li className={xray.lz77.strings > 0 ? "warning" : undefined}>
              {xray.lz77.blocks > 0
                ? t("xray.lz77", {
                    blocks: xray.lz77.blocks,
                    packed: formatBytes(xray.lz77.packedBytes),
                    unpacked: formatBytes(xray.lz77.unpackedBytes),
                    withText: xray.lz77.blocksWithText,
                    strings: xray.lz77.strings,
                  })
                : t("xray.lz77None")}
              {xray.lz77.truncated && ` ${t("xray.lz77Truncated")}`}
            </li>
            <li>
              {t("xray.dense", {
                dense: Math.round((100 * xray.denseBytes) / Math.max(xray.size, 1)),
                blank: Math.round((100 * xray.blankBytes) / Math.max(xray.size, 1)),
              })}
            </li>
          </ul>
        </div>
      )}

      {scanInfo && (
        <div className="ok">
          {t("entries.saved", { n: scanInfo.found })}
          {scanInfo.truncated && ` — ${t("extract.truncated")}`}
        </div>
      )}
      {scanInfo && scanInfo.discarded > 0 && (
        <p className="hint left">
          {t(scanInfo.structured ? "extract.discardedStructured" : "extract.discarded", {
            n: scanInfo.discarded,
          })}
        </p>
      )}
      {scanInfo && scanInfo.removed > 0 && (
        <p className="hint left">{t("extract.removedOld", { n: scanInfo.removed })}</p>
      )}
      {scanInfo && scanInfo.stray > 0 && (
        <p className="hint left">{t("extract.stray", { n: scanInfo.stray })}</p>
      )}
      {scanInfo && scanInfo.packed > 0 && (
        <p className="hint left">{t("extract.packed", { n: scanInfo.packed })}</p>
      )}
      {scanInfo && scanInfo.jis16 > 0 && (
        <p className="hint left">{t("extract.jis16", { n: scanInfo.jis16 })}</p>
      )}
      {savedPath && <div className="ok">{t("export.saved", { path: savedPath })}</div>}

      {entries.length === 0 && (
        <div className="empty">
          <strong>{t("extract.emptyTitle")}</strong>
          {t("extract.emptyHint")}
        </div>
      )}

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
                {shown.length === 0 && (
                  <tr>
                    <td className="empty-row" colSpan={hasResources ? 5 : 4}>
                      {t("extract.none")}
                    </td>
                  </tr>
                )}
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
              onUnmarkInternal={() => unmarkInternal(selected)}
            />
          ) : (
            <p className="empty">{t("editor.pick")}</p>
          )}
        </>
      )}

      {settings && endpoint && (
        <div className="panel">
          <h3 data-step="2">{t("translate.title")}</h3>
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
                <option value="anthropic">{t("translate.providerClaudeKey")}</option>
                <option value="codex_cli">ChatGPT (Codex CLI)</option>
              </select>
            </label>
            {settings.provider === "codex_cli" ? (
              <>
                <label>
                  {t("translate.codexCommand")}
                  <input
                    type="text"
                    size={28}
                    value={settings.codexCli.command}
                    placeholder="codex"
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        codexCli: { ...settings.codexCli, command: e.target.value },
                      })
                    }
                  />
                </label>
                <label>
                  {t("translate.codexModel")}
                  <input
                    type="text"
                    size={18}
                    value={settings.codexCli.model}
                    placeholder={t("translate.codexModelDefault")}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        codexCli: { ...settings.codexCli, model: e.target.value },
                      })
                    }
                  />
                </label>
              </>
            ) : (
              <>
                {settings.provider !== "anthropic" && (
                  <label>
                    {t("translate.baseUrl")}
                    <input
                      type="text"
                      size={28}
                      value={endpoint.baseUrl}
                      onChange={(e) => updateEndpoint(endpointKey, "baseUrl", e.target.value)}
                    />
                  </label>
                )}
                <label>
                  {t("translate.model")}
                  <input
                    type="text"
                    size={18}
                    value={endpoint.model}
                    placeholder={
                      settings.provider === "ollama"
                        ? "llama3.2:3b"
                        : settings.provider === "anthropic"
                          ? "claude-opus-5-5"
                          : "gpt-5-mini"
                    }
                    onChange={(e) => updateEndpoint(endpointKey, "model", e.target.value)}
                  />
                </label>
              </>
            )}
            <label title={t("translate.batchSizeHint")}>
              {t("translate.batchSize")}
              <input
                type="number"
                min={1}
                max={50}
                value={settings.batchSize}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    batchSize: Math.min(50, Math.max(1, Number(e.target.value) || 1)),
                  })
                }
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
            {settings.provider === "anthropic" && (
              <label>
                {t("translate.apiKey")}{" "}
                {anthropicKeySet && <em>{t("translate.apiKeySet")}</em>}
                <input
                  type="password"
                  size={22}
                  value={anthropicKeyInput}
                  placeholder="sk-ant-..."
                  onChange={(e) => setAnthropicKeyInput(e.target.value)}
                />
              </label>
            )}
          </div>
          {settings.provider === "anthropic" && (
            <p className="hint left">{t("translate.claudeKeyHint")}</p>
          )}
          {paidByToken && (
            <>
              <div className="scan-controls">
                <label title={t("translate.priceHint")}>
                  {t("translate.priceInput")}
                  <input
                    type="number"
                    min={0}
                    step="0.01"
                    value={settings.priceInput}
                    onChange={(e) =>
                      setSettings({ ...settings, priceInput: Math.max(0, Number(e.target.value) || 0) })
                    }
                  />
                </label>
                <label title={t("translate.priceHint")}>
                  {t("translate.priceOutput")}
                  <input
                    type="number"
                    min={0}
                    step="0.01"
                    value={settings.priceOutput}
                    onChange={(e) =>
                      setSettings({ ...settings, priceOutput: Math.max(0, Number(e.target.value) || 0) })
                    }
                  />
                </label>
                <button onClick={runEstimate} disabled={estimating || translating}>
                  {estimating ? t("translate.estimating") : t("translate.estimate")}
                </button>
              </div>
              <p className="hint left">
                {prices
                  ? t("translate.priceUsed", { input: prices[0], output: prices[1] })
                  : t("translate.priceMissing")}
              </p>
              {estimate && (
                <div className="warn">
                  {t("translate.estimateResult", {
                    entries: estimate.entries,
                    batches: estimate.batches,
                    input: estimate.inputTokens.toLocaleString(),
                    output: estimate.outputTokens.toLocaleString(),
                  })}
                  {dollars(estimate.inputTokens, estimate.outputTokens) !== null &&
                    ` ${t("translate.estimateCost", {
                      cost: money(dollars(estimate.inputTokens, estimate.outputTokens) ?? 0),
                    })}`}
                  <br />
                  {estimate.calibrated
                    ? t("translate.estimateCalibrated")
                    : t("translate.estimateRough")}
                </div>
              )}
            </>
          )}
          {settings.provider === "codex_cli" && (
            <>
              <p className="hint left">{t("translate.codexHint")}</p>
              <div className="scan-controls">
                <label>
                  {t("translate.codexAccount")}
                  <select
                    value={settings.codexCli.ownAccount ? "app" : "system"}
                    disabled={loggingIn}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        codexCli: { ...settings.codexCli, ownAccount: e.target.value === "app" },
                      })
                    }
                  >
                    <option value="system">{t("translate.codexAccountSystem")}</option>
                    <option value="app">{t("translate.codexAccountApp")}</option>
                  </select>
                </label>
                {settings.codexCli.ownAccount && !loggingIn && (
                  <>
                    <button onClick={() => codexLogin(codexLogged === true)}>
                      {codexLogged ? t("translate.codexSwitch") : t("translate.codexLogin")}
                    </button>
                    {codexLogged && <button onClick={codexLogout}>{t("translate.codexLogout")}</button>}
                  </>
                )}
                {loggingIn && (
                  <button onClick={() => invoke("codex_login_cancel")}>
                    {t("translate.codexLoginCancel")}
                  </button>
                )}
              </div>
              <p className="hint left">
                {loggingIn
                  ? t("translate.codexLoginWaiting")
                  : codexLogged === null
                    ? t("translate.codexChecking")
                    : codexLogged
                      ? t("translate.codexLogged")
                      : settings.codexCli.ownAccount
                        ? t("translate.codexNotLoggedApp")
                        : t("translate.codexNotLoggedSystem")}
              </p>
            </>
          )}
          {hasAscii && <p className="hint left">{t("translate.ascii")}</p>}
          {hasJapanese && <p className="hint left">{t("translate.japanese")}</p>}
          {hasJis16 && <p className="hint left">{t("translate.jis16")}</p>}
          {internal > 0 && <p className="hint left">{t("translate.internal", { n: internal })}</p>}
          {settings.provider !== "ollama" && (
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
            <button
              onClick={async () => {
                if (await saveSettings()) setTestResult(t("translate.saved"));
              }}
            >
              {t("translate.saveSettings")}
            </button>
            <button onClick={() => testProvider()} disabled={testing || translating}>
              {testing ? t("translate.testing") : t("translate.test")}
            </button>
            {translating ? (
              <button onClick={cancelTranslation} disabled={cancelling}>
                {cancelling ? t("translate.cancelling") : t("translate.cancel")}
              </button>
            ) : (
              <button
                className="primary"
                onClick={() => runTranslation()}
                disabled={pending === 0 || sourceBroken}
              >
                {t("translate.run")} ({pending})
              </button>
            )}
          </div>
          {remoteAsk && (
            <div className="warn" role="alert">
              {t("translate.remoteAsk")}
              <div className="actions">
                <button onClick={() => setRemoteAsk(null)}>{t("translate.cancel")}</button>
                <button className="primary" onClick={allowRemoteAndContinue}>
                  {t("translate.remoteAllow")}
                </button>
              </div>
            </div>
          )}
          {translateError && (
            <div className="error" role="alert">
              <strong>{t("common.error")}:</strong> {translateError}
            </div>
          )}
          {testResult && <div className="ok">{testResult}</div>}
          {translating && (
            <div className="progress-row">
              <progress value={progress?.done ?? 0} max={progress?.total ?? 1} />
              <span>
                {cancelling
                  ? t("translate.cancelling")
                  : progress
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
              {summary.shortened > 0 &&
                t("translate.summaryShortened", { shortened: summary.shortened })}
              {summary.tooLong > 0 && t("translate.summaryTooLong", { tooLong: summary.tooLong })}
              {summary.inputTokens + summary.outputTokens > 0 && (
                <>
                  <br />
                  {t("translate.spent", {
                    input: summary.inputTokens.toLocaleString(),
                    output: summary.outputTokens.toLocaleString(),
                  })}
                  {dollars(summary.inputTokens, summary.outputTokens) !== null &&
                    ` ${t("translate.spentCost", {
                      cost: money(dollars(summary.inputTokens, summary.outputTokens) ?? 0),
                    })}`}
                </>
              )}
            </div>
          )}
        </div>
      )}

      {entries.length > 0 && (
        <div className="panel">
          <h3 data-step="3">{t("reinsert.title")}</h3>
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
          {reinsertError && (
            <div className="error" role="alert">
              <strong>{t("common.error")}:</strong> {reinsertError}
            </div>
          )}
          {reinsertOutcome && (
            <>
              <div className="ok">
                {t("reinsert.done")}
                <span className="path">{reinsertOutcome.workingPath}</span>
                <button
                  onClick={() =>
                    revealItemInDir(reinsertOutcome.workingPath).catch((e) =>
                      setError(String(e)),
                    )
                  }
                >
                  {t("reinsert.reveal")}
                </button>
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
                {reinsertOutcome.apply.packedLeftOut > 0 && (
                  <>
                    <br />
                    {t("reinsert.packedLeftOut", { n: reinsertOutcome.apply.packedLeftOut })}
                  </>
                )}
              </div>
              <p className="hint left">{t("reinsert.personal")}</p>
              <ul className="evidence">
                {reinsertOutcome.verification.checks.map((c) => (
                  <li key={c}>{c}</li>
                ))}
              </ul>
              {reinsertOutcome.skippedErrors > 0 && (
                <div className="warn">
                  {t("reinsert.skipped", { n: reinsertOutcome.skippedErrors })}
                </div>
              )}
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
          <p className="empty">{t("glossary.empty")}</p>
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
  onUnmarkInternal: () => void;
}

function EntryEditor({
  entry,
  draft,
  issues,
  t,
  onDraft,
  onSave,
  onToggleReviewed,
  onUnmarkInternal,
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

      {entry.status === "internal" && <p className="hint left">{t("editor.internal")}</p>}

      <div className="actions">
        {entry.status === "internal" && (
          <button onClick={onUnmarkInternal}>{t("editor.unmarkInternal")}</button>
        )}
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
