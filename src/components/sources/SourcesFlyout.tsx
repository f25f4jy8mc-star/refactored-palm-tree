// The watched folders, and the only place they're managed: add, disable,
// stop watching, re-index.
//
// Two things worth knowing while using it, both stated in the panel
// itself rather than left to be discovered:
//
//   * Re-index always covers every enabled source in one pass. It can't
//     be per-folder: a scan finishes by marking everything it didn't see
//     as missing, so a partial walk would declare the skipped folders gone.
//   * Stop watching keeps the items by default. Their tags, links and notes
//     are the user's work; unwatching a folder stops it being refreshed, it
//     does not throw that away. Holding ⌥ while clicking ✕ forgets them too,
//     which is the answer to "I unwatched everything and the content is
//     still here" — it was doing exactly what it said, and there was no way
//     to ask for the other thing.
//   * Empty library is the blunt version of that: every item goes, the
//     watched folders stay. Which means a re-index brings it all back, and
//     the button says so before it asks.

import { useCallback, useEffect, useState } from "react";

import {
  addSource,
  clearLibrary,
  listSources,
  pickFolder,
  removeSource,
  rescan,
  setSourceEnabled,
} from "../../lib/api";
import { useArchivaChanged } from "../../lib/events";
import type { Source } from "../../lib/types";

export function SourcesFlyout({ onClose }: { onClose: () => void }) {
  const [sources, setSources] = useState<Source[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Emptying the library is not undoable, so it asks once rather than
  // firing off a click that lands next to an unrelated ✕.
  const [confirmClear, setConfirmClear] = useState(false);

  const load = useCallback(async () => {
    try {
      setSources(await listSources());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useArchivaChanged(load);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  async function run(label: string, fn: () => Promise<unknown>) {
    setBusy(label);
    setError(null);
    try {
      await fn();
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function onAdd() {
    const dir = await pickFolder();
    if (!dir) return;
    await run(`Indexing ${dir}…`, () => addSource(dir));
  }

  return (
    <div className="flyout" onClick={(e) => e.stopPropagation()}>
      <div className="flyout-head">
        <strong>Sources</strong>
        <span className="hint">files stay where they are</span>
      </div>

      {sources.length === 0 && !busy && (
        <p className="hint">No folders watched yet.</p>
      )}

      <ul className="source-list">
        {sources.map((s) => (
          <li key={s.id} className={s.enabled ? "" : "off"}>
            <input
              type="checkbox"
              checked={s.enabled}
              title={s.enabled ? "Stop including in scans" : "Include in scans"}
              onChange={(e) => run("Updating…", () => setSourceEnabled(s.id, e.target.checked))}
            />
            <span className="source-path" title={s.path}>
              {s.path}
            </span>
            <span className="source-count">{s.item_count}</span>
            <button
              className="btn quiet"
              title={
                "Stop watching — indexed items remain.\n" +
                "Hold ⌥ to forget its items as well (files are never touched)."
              }
              onClick={(e) =>
                run(
                  e.altKey ? "Removing and forgetting…" : "Removing…",
                  () => removeSource(s.id, e.altKey),
                )
              }
            >
              ✕
            </button>
          </li>
        ))}
      </ul>

      <div className="flyout-actions">
        <button className="btn primary" onClick={onAdd} disabled={!!busy}>
          Add Folder…
        </button>
        <button
          className="btn"
          onClick={() => run("Re-indexing…", rescan)}
          disabled={!!busy || sources.every((s) => !s.enabled)}
          title="Walk every enabled source and reconcile what changed"
        >
          Re-index
        </button>
      </div>

      <div className="flyout-danger">
        {confirmClear ? (
          <>
            <p className="hint">
              Every indexed item goes — tags, links and notes with them. Files on disk are not
              touched, and the folders above stay watched, so a re-index brings the items back
              without what you had added to them.
            </p>
            <div className="flyout-actions">
              <button className="btn" onClick={() => setConfirmClear(false)} disabled={!!busy}>
                Cancel
              </button>
              <button
                className="btn primary"
                onClick={() =>
                  run("Emptying…", async () => {
                    await clearLibrary();
                    setConfirmClear(false);
                  })
                }
                disabled={!!busy}
              >
                Empty the library
              </button>
            </div>
          </>
        ) : (
          <button
            className="btn quiet"
            onClick={() => setConfirmClear(true)}
            disabled={!!busy}
            title="Remove every indexed item. Files are not touched."
          >
            Empty library…
          </button>
        )}
      </div>

      <div className="flyout-status">
        {error ? <span className="error">{error}</span> : busy}
      </div>
    </div>
  );
}
