import { makeT, MessageId } from "./i18n";
import { formatOffset } from "./util";
import { AuditReport, LeftoverWhy } from "./types";

/** Quantos trechos a tela lista (o resto fica na conta por motivo e por região). */
const LISTED = 50;

/**
 * Auditoria: a cópia traduzida relida com a mesma extração do app, e o que
 * segue no idioma de origem, com o motivo. Clicar num trecho abre a frase.
 */
export default function AuditPanel({
  audit,
  running,
  error,
  disabled,
  t,
  onRun,
  onPick,
}: {
  audit: AuditReport | null;
  running: boolean;
  error: string | null;
  disabled: boolean;
  t: ReturnType<typeof makeT>;
  onRun: () => void;
  /** A frase do projeto com esse id, se houver. */
  onPick: (id: string) => void;
}) {
  const byWhy = new Map<LeftoverWhy, [number, number]>();
  const byRegion = new Map<number, [number, number]>();
  for (const left of audit?.remaining ?? []) {
    const why = byWhy.get(left.why) ?? [0, 0];
    byWhy.set(left.why, [why[0] + 1, why[1] + left.chars]);
    if (left.offset !== null) {
      const at = Math.floor(left.offset / 0x10000) * 0x10000;
      const region = byRegion.get(at) ?? [0, 0];
      byRegion.set(at, [region[0] + 1, region[1] + left.chars]);
    }
  }
  const regions = [...byRegion].sort((a, b) => b[1][1] - a[1][1]).slice(0, 10);
  const done = audit ? audit.sourceChars - audit.remainingChars : 0;
  const percent = audit && audit.sourceChars > 0 ? (100 * done) / audit.sourceChars : 100;
  const clean = audit && audit.remainingChars === 0 && audit.pictures.unchecked.length === 0;

  return (
    <div className="panel" id="step-audit">
      <h3 data-step="6">{t("audit.title")}</h3>
      <p className="hint left">{t("audit.hint")}</p>
      <div className="actions">
        <button className="primary" onClick={onRun} disabled={running || disabled}>
          {running ? t("audit.running") : t("audit.run")}
        </button>
      </div>
      {error && (
        <div className="error" role="alert">
          <strong>{t("common.error")}:</strong> {error}
        </div>
      )}
      {audit && (
        <div className={clean ? "ok" : "warn"}>
          {clean
            ? t("audit.done")
            : t("audit.summary", {
                remaining: audit.remainingChars.toLocaleString(),
                total: audit.sourceChars.toLocaleString(),
                percent: percent.toFixed(2),
              })}
          {[...byWhy].map(([why, [n, chars]]) => (
            <div key={why}>
              {t("audit.whyCount", { why: t(`audit.why.${why}` as MessageId), n, chars })}
            </div>
          ))}
          <div>
            {t("audit.pictures", {
              found: audit.pictures.found,
              read: audit.pictures.read,
              without: audit.pictures.withoutText,
              unchecked: audit.pictures.unchecked.length,
            })}
          </div>
          {audit.pictures.unchecked.length > 0 && (
            <div className="mono">
              {t("audit.unchecked", { ids: audit.pictures.unchecked.join(", ") })}
            </div>
          )}
        </div>
      )}
      {audit && audit.notText > 0 && (
        <p className="hint left">{t("audit.notText", { n: audit.notText })}</p>
      )}
      {regions.length > 0 && (
        <>
          <p className="hint left">{t("audit.regions")}</p>
          <ul className="issues">
            {regions.map(([at, [n, chars]]) => (
              <li key={at}>{t("audit.region", { at: formatOffset(at), n, chars })}</li>
            ))}
          </ul>
        </>
      )}
      {audit && audit.remaining.length > 0 && (
        <>
          <p className="hint left">
            {t("audit.leftovers", { shown: Math.min(LISTED, audit.remaining.length) })}
          </p>
          <div className="strings-wrap">
            <table className="strings">
              <tbody>
                {audit.remaining.slice(0, LISTED).map((left) => (
                  <tr key={`${left.id}-${left.offset}`} className="row" onClick={() => onPick(left.id)}>
                    <td className="mono">{formatOffset(left.offset)}</td>
                    <td className="text-cell">{left.text}</td>
                    <td>{t(`audit.why.${left.why}` as MessageId)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  );
}
