import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  disabled,
}: {
  projectDir: string;
  t: ReturnType<typeof makeT>;
  /** Depois da leitura: as frases novas entram na lista. */
  onRead: () => Promise<void> | void;
  disabled: boolean;
}) {
  const [pictures, setPictures] = useState<Picture[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(false);
  const [reading, setReading] = useState(false);
  const [outcome, setOutcome] = useState<ReadPicturesOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function find() {
    setBusy(true);
    setError(null);
    setOutcome(null);
    try {
      setPictures(await invoke<Picture[]>("list_pictures", { projectDir }));
      setPage(0);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function read() {
    setReading(true);
    setError(null);
    try {
      const result = await invoke<ReadPicturesOutcome>("read_pictures", {
        projectDir,
        ids: [...selected],
      });
      setOutcome(result);
      await onRead();
    } catch (e) {
      setError(String(e));
    } finally {
      setReading(false);
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
    <div className="panel">
      <h3>{t("pictures.title")}</h3>
      <p className="hint left">{t("pictures.hint")}</p>
      <div className="actions">
        <button onClick={find} disabled={busy || disabled}>
          {busy ? t("pictures.finding") : t("pictures.find")}
        </button>
        {pictures && (
          <button
            className="primary"
            onClick={read}
            disabled={reading || disabled || selected.size === 0}
            title={t("pictures.readHint")}
          >
            {reading ? t("pictures.reading") : t("pictures.read", { n: selected.size })}
          </button>
        )}
      </div>
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
                  {formatOffset(p.offset)} · {p.packed !== null ? "LZ77" : t("pictures.raw")}
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

/** A folha da imagem, carregada do projeto quando aparece. */
function Thumb({ projectDir, id }: { projectDir: string; id: string }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    let made: string | null = null;
    invoke<number[]>("picture_png", { projectDir, id, reading: false })
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
  }, [projectDir, id]);
  return url ? <img src={url} alt={id} /> : <div className="picture-empty" />;
}
