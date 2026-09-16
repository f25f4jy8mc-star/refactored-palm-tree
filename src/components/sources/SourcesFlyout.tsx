// The watched folders, and the only place they're managed: add, disable,
// stop watching, re-index.
//
// Two things worth knowing while using it, both stated in the panel
// itself rather than left to be discovered:
//
//   * Re-index always covers every enabled source in one pass. It can't
//     be per-folder: a scan finishes by marking everything it didn't see
//     as missing, so a partial walk would declare the skipped folders gone.
//   * The tickbox includes or excludes a folder from what is *displayed*,
//     and it is staged: nothing moves until Refresh. That is what makes
//     Refresh the moment the library changes, rather than content vanishing
//     under the pointer as you work down a list. Unticking hides everything
//     under it — from the Library, the columns and search alike — and
//     ticking it again brings the lot back with its tags intact. Nothing is
//     scanned or forgotten either way.
//   * Displaying the linked folders is not staged, because nothing is
//     indexed or forgotten by it: it only decides whether the folders
//     themselves are drawn in the Library alongside what is in them. They
//     get their own heading there — a folder you mirrored is not a collector
//     you made.
//   * Unlinking asks which of two things you mean, because they are not the
//     same and neither is the obvious default: keep the links and tags you
//     added to those items, or delete them with the folder. Files on disk
//     are never touched by either.
//   * Empty library is the blunt version of that: every item goes, the
//     watched folders stay. Which means a re-index brings it all back, and
//     the button says so before it asks.

import { useCallback, useEffect, useState } from "react";

import {
  addSource,
  clearLibrary,
  getSettings,
  isStaged,
  listSources,
  pickFolder,
  removeSource,
  rescan,
  setShowLinkedFolders,
  setSourceEnabled,
  tickOf,
} from "../../lib/api";
import { useArchivaChanged } from "../../lib/events";
import type { Settings, Source } from "../../lib/types";
import { SpacesPanel } from "../spaces/SpacesPanel";

export function SourcesFlyout({ onClose }: { onClose: () => void }) {
  const [sources, setSources] = useState<Source[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Emptying the library is not undoable, so it asks once rather than
  // firing off a click that lands next to an unrelated ✕.
  const [confirmClear, setConfirmClear] = useState(false);
  // Unlinking asks which of two things is meant. The two answers differ by
  // what happens to work the user did — that is not a question to answer for
  // them with a modifier key nobody finds.
  const [unlinking, setUnlinking] = useState<Source | null>(null);

  const load = useCallback(async () => {
    try {
      const [list, prefs] = await Promise.all([listSources(), getSettings()]);
      setSources(list);
      setSettings(prefs);
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

  const staged = isStaged(sources);

  async function onAdd() {
    const dir = await pickFolder();
    if (!dir) return;
    await run(`Indexing ${dir}…`, () => addSource(dir));
  }

  return (
    <div className="flyout" onClick={(e) => e.stopPropagation()}>
      <SpacesPanel onBusy={(b) => setBusy(b ? "Working…" : null)} />

      <div className="flyout-head">
        <h2>Linked folders</h2>
        <span className="hint">files stay where they are</span>
      </div>

      {sources.length === 0 && !busy && (
        <p className="hint">No folders watched yet.</p>
      )}

      <ul className="source-list">
        {sources.map((s) => (
          // `off` follows what is in effect, so the list keeps showing the
          // library as it stands; `staged` marks the lines that a Refresh
          // would change. The two are deliberately different marks.
          <li
            key={s.id}
            className={[s.enabled ? "" : "off", s.pending_enabled === null ? "" : "staged"]
              .filter(Boolean)
              .join(" ")}
          >
            <input
              type="checkbox"
              checked={tickOf(s)}
              title={
                tickOf(s)
                  ? "Exclude this folder from what is displayed, on the next Refresh"
                  : "Include this folder in what is displayed, on the next Refresh"
              }
              onChange={(e) => run("Staging…", () => setSourceEnabled(s.id, e.target.checked))}
            />
            <span className="source-path" title={s.path}>
              {s.path}
            </span>
            {s.pending_enabled === null ? (
              <span className="source-count">{s.item_count}</span>
            ) : (
              <span className="source-pending" title="Waiting on a Refresh">
                {s.pending_enabled ? "will show" : "will hide"}
              </span>
            )}
            <button
              className="btn quiet"
              title="Unlink this folder"
              onClick={() => setUnlinking(s)}
            >
              ✕
            </button>
          </li>
        ))}
      </ul>

      {sources.length > 0 && (
        <label className="source-option" title="Draw the folders themselves, not just what is in them">
          <input
            type="checkbox"
            checked={settings?.showLinkedFolders ?? false}
            disabled={!settings || !!busy}
            onChange={(e) =>
              run("Updating…", () => setShowLinkedFolders(e.target.checked))
            }
          />
          <span>Display linked folders in the Library</span>
        </label>
      )}

      {staged ? (
        <p className="hint staged">
          A tickbox changed. Nothing moves until you Refresh — that is the one
          moment the library changes, rather than content going as you click.
        </p>
      ) : (
        sources.some((s) => !s.enabled) && (
          <p className="hint">
            Unticked folders are hidden everywhere — the Library, the columns and
            search. Their items, tags and links are untouched, and ticking them
            back brings the lot back. Refresh re-reads the folders that are on.
          </p>
        )
      )}

      <div className="flyout-actions">
        <button className={staged ? "btn" : "btn primary"} onClick={onAdd} disabled={!!busy}>
          Add Folder…
        </button>
        <button
          className={staged ? "btn primary" : "btn"}
          onClick={() => run("Refreshing…", rescan)}
          // Available whenever there is something to apply, even when that
          // something is switching the last folder off.
          disabled={!!busy || (!staged && sources.every((s) => !s.enabled))}
          title={
            staged
              ? "Apply the tickboxes, then walk every included folder"
              : "Walk every included folder and reconcile what changed"
          }
        >
          Refresh
        </button>
      </div>

      {unlinking && (
        <div className="confirm">
          <div className="confirm-what">
            Unlink <b>{unlinking.path}</b>?
          </div>
          <p className="hint">
            Archiva stops watching the folder either way, and the files on disk
            are never touched. The question is what happens to the work you did
            on its {unlinking.item_count} item{unlinking.item_count === 1 ? "" : "s"} — the
            tags you applied and the links you drew, which live in this space,
            not in the folder.
          </p>
          <div className="confirm-actions">
            <button className="btn" onClick={() => setUnlinking(null)} disabled={!!busy}>
              Cancel
            </button>
            <button
              className="btn"
              title="The items stay in the library with their tags and links"
              disabled={!!busy}
              onClick={() => {
                const id = unlinking.id;
                setUnlinking(null);
                run("Unlinking…", () => removeSource(id, false));
              }}
            >
              Unlink, keep tags and links
            </button>
            <button
              className="btn primary"
              title="The items and everything added to them are forgotten"
              disabled={!!busy}
              onClick={() => {
                const id = unlinking.id;
                setUnlinking(null);
                run("Unlinking and forgetting…", () => removeSource(id, true));
              }}
            >
              Unlink and delete them
            </button>
          </div>
        </div>
      )}

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
