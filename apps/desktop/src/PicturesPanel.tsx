import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { makeT } from "./i18n";
import { formatOffset } from "./util";
import { Picture, ReadPicturesOutcome } from "./types";

/** Quantas miniaturas por página: a ROM pode ter centenas de imagens. */
const PAGE = 24;

/**
 * Texto desenhado em imagem: o app acha as imagens com letra (sem IA), quem
 * usa marca as que têm japonês, e só essas vão pra IA ler. Cada trecho com
 * japonês vira uma frase do projeto, traduzida como as outras e redesenhada
 * na reinserção.
 */
export default function PicturesPanel({
  projectDir,
  t,
  onRead,
  onChecked,
  onError,
  disabled,
}: {
  projectDir: string;
  t: ReturnType<typeof makeT>;
  /** Depois da leitura: as frases novas entram na lista. */
  onRead: () => Promise<void> | void;
  /** Imagens lidas ou marcadas sem texto: a auditoria para de cobrar por elas. */
  onChecked: (ids: string[]) => void;
  /** O painel mostra um erro (o passo a passo aponta pra ca). */
  onError: (failed: boolean) => void;
  disabled: boolean;
}) {
  const [pictures, setPictures] = useState<Picture[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(false);
  const [reading, setReading] = useState(false);
  const [outcome, setOutcome] = useState<ReadPicturesOutcome | null>(null);
  const [marked, setMarked] = useState<number | null>(null);
  // Folhas de trechos gravadas agora; a versao recarrega a miniatura.
  const [sheets, setSheets] = useState<{ ids: string[]; version: number } | null>(null);
  const [error, setErrorText] = useState<string | null>(null);
  const setError = (message: string | null) => {
    setErrorText(message);
    onError(message !== null);
  };

  async function find() {
    setBusy(true);
    setError(null);
    setOutcome(null);
    setMarked(null);
    setSheets(null);
    try {
      setPictures(await invoke<Picture[]>("list_pictures", { projectDir }));
      setPage(0);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  /**
   * Le as marcadas: pela IA, de um arquivo (`answerPath`, uma imagem) ou so a
   * folha de trechos que iria pra IA (`preview`), sem chamar ninguem.
   */
  async function read(mode: { preview?: boolean; answerPath?: string } = {}) {
    const ids = [...selected];
    setReading(true);
    setError(null);
    setMarked(null);
    try {
      const result = await invoke<ReadPicturesOutcome>("read_pictures", {
        projectDir,
        ids,
        preview: mode.preview ?? false,
        answerPath: mode.answerPath ?? null,
      });
      if (mode.preview) {
        setSheets({ ids, version: (sheets?.version ?? 0) + 1 });
        return;
      }
      setOutcome(result);
      const failed = new Set(result.failed.map(([id]) => id));
      onChecked(ids.filter((id) => !failed.has(id)));
      await onRead();
    } catch (e) {
      setError(String(e));
    } finally {
      setReading(false);
    }
  }

  async function readFromFile() {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof path === "string") await read({ answerPath: path });
  }

  async function markWithoutText() {
    const ids = [...selected];
    setError(null);
    setOutcome(null);
    try {
      await invoke<number>("mark_pictures_without_text", { projectDir, ids });
      setMarked(ids.length);
      onChecked(ids);
    } catch (e) {
      setError(String(e));
    }
  }

  function toggle(id: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  const shown = pictures?.slice(page * PAGE, (page + 1) * PAGE) ?? [];
  const pages = pictures ? Math.ceil(pictures.length / PAGE) : 0;

  return (
    <div className="panel" id="step-pictures">
      <h3 data-step="4">{t("pictures.title")}</h3>
      <p className="hint left">{t("pictures.hint")}</p>
      <div className="actions">
        <button onClick={find} disabled={busy || disabled}>
          {busy ? t("pictures.finding") : t("pictures.find")}
        </button>
        {pictures && (
          <>
            <button
              onClick={markWithoutText}
              disabled={reading || disabled || selected.size === 0}
              title={t("pictures.withoutTextHint")}
            >
              {t("pictures.withoutText", { n: selected.size })}
            </button>
            <button
              onClick={() => read({ preview: true })}
              disabled={reading || disabled || selected.size === 0}
              title={t("pictures.previewHint")}
            >
              {t("pictures.preview")}
            </button>
            <button
              onClick={readFromFile}
              disabled={reading || disabled || selected.size !== 1}
              title={t("pictures.fromFileHint")}
            >
              {t("pictures.fromFile")}
            </button>
            <button
              className="primary"
              onClick={() => read()}
              disabled={reading || disabled || selected.size === 0}
              title={t("pictures.readHint")}
            >
              {reading ? t("pictures.reading") : t("pictures.read", { n: selected.size })}
            </button>
          </>
        )}
      </div>
      {pictures && pictures.length > 0 && (
        <p className="hint left">{t("pictures.outsideHint")}</p>
      )}
      {error && (
        <div className="error" role="alert">
          <strong>{t("common.error")}:</strong> {error}
        </div>
      )}
      {outcome && (
        <div className={outcome.failed.length > 0 ? "warn" : "ok"}>
          {t("pictures.readDone", {
            pictures: outcome.pictures,
            blocks: outcome.blocks,
            entries: outcome.entries,
          })}
          {outcome.failed.map(([id, why]) => (
            <div key={id}>
              {id}: {why}
            </div>
          ))}
        </div>
      )}
      {marked !== null && <div className="ok">{t("pictures.withoutTextDone", { n: marked })}</div>}
      {sheets && (
        <div className="pictures">
          {sheets.ids.map((id) => (
            <figure key={`${id}-${sheets.version}`} className="picture">
              <Thumb projectDir={projectDir} id={id} reading />
              <figcaption className="hint">{t("pictures.sheet", { id })}</figcaption>
            </figure>
          ))}
        </div>
      )}
      {pictures && pictures.length === 0 && <p className="hint left">{t("pictures.none")}</p>}
      {pictures && pictures.length > 0 && (
        <>
          <p className="hint left">{t("pictures.found", { n: pictures.length })}</p>
          <div className="pictures">
            {shown.map((p) => (
              <label key={p.id} className={selected.has(p.id) ? "picture chosen" : "picture"}>
                <input
                  type="checkbox"
                  checked={selected.has(p.id)}
                  onChange={() => toggle(p.id)}
                />
                <Thumb projectDir={projectDir} id={p.id} />
                <span className="hint">
                  {p.map
                    ? `${formatOffset(p.map.offset)} · ${t("pictures.map")}`
                    : `${formatOffset(p.offset)} · ${p.packed !== null ? "LZ77" : t("pictures.raw")}`}
                </span>
              </label>
            ))}
          </div>
          {pages > 1 && (
            <div className="actions">
              <button onClick={() => setPage(page - 1)} disabled={page === 0}>
                ‹
              </button>
              <span className="hint">
                {page + 1} / {pages}
              </span>
              <button onClick={() => setPage(page + 1)} disabled={page + 1 >= pages}>
                ›
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** A folha da imagem (ou a de trechos, `reading`), carregada do projeto quando aparece. */
function Thumb({
  projectDir,
  id,
  reading = false,
}: {
  projectDir: string;
  id: string;
  reading?: boolean;
}) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    let made: string | null = null;
    invoke<number[]>("picture_png", { projectDir, id, reading })
      .then((bytes) => {
        if (!alive) return;
        made = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
        setUrl(made);
      })
      .catch(() => setUrl(null));
    return () => {
      alive = false;
      if (made) URL.revokeObjectURL(made);
    };
  }, [projectDir, id, reading]);
  return url ? <img src={url} alt={id} /> : <div className="picture-empty" />;
}
