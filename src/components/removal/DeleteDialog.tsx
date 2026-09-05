// Removing items, and the one question that has to be asked first.
//
// There is no undo yet (⌘Z is on the deferred list), so this counts what is
// about to go before anything goes, and names the consequence of each choice
// rather than leaving it to be discovered:
//
//   * **Remove from library** forgets the rows. The files stay exactly where
//     they are — which means a file still inside a watched folder is indexed
//     again on the next scan, as a new item with none of its tags. That is
//     what "forget, don't delete" costs, and it is said here rather than
//     looking like the delete silently failed.
//   * **Move files to Archiva's trash** takes the file out of the watched
//     folder first, so it stays gone, and leaves it on disk so a mistake is
//     recoverable.
//
// Deleting a Collector releases its members rather than taking them. The
// count is shown, because "delete this folder" reading as "delete these
// hundred photographs" is the mistake worth spending a line to prevent.

import { useEffect, useState } from "react";

import { deleteItems, previewRemoval } from "../../lib/api";
import type { RemovalPreview } from "../../lib/types";

export function DeleteDialog({
  ids,
  onClose,
}: {
  ids: string[];
  onClose: (removed: number) => void;
}) {
  const [preview, setPreview] = useState<RemovalPreview | null>(null);
  const [trashFiles, setTrashFiles] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    previewRemoval(ids).then(setPreview).catch((e) => setError(String(e)));
  }, [ids]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // The dialog owns Escape while it is up, or it closes the pane behind.
      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();
      onClose(0);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  async function confirm() {
    setBusy(true);
    try {
      const out = await deleteItems(ids, trashFiles);
      if (out.failed.length > 0) {
        // A partial failure must never read as a success: the rows for those
        // files were kept, and saying so is the whole point.
        setError(
          `${out.failed.length} file${out.failed.length === 1 ? "" : "s"} could not be moved and ` +
            `${out.failed.length === 1 ? "was" : "were"} left alone — ${out.failed.join("; ")}`,
        );
        setBusy(false);
        return;
      }
      onClose(out.forgotten);
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  const parts: string[] = [];
  if (preview) {
    if (preview.items) parts.push(`${preview.items} item${preview.items === 1 ? "" : "s"}`);
    if (preview.notes) parts.push(`${preview.notes} note${preview.notes === 1 ? "" : "s"}`);
    if (preview.collectors)
      parts.push(`${preview.collectors} collector${preview.collectors === 1 ? "" : "s"}`);
  }

  return (
    <div className="dialog-backdrop" onClick={() => onClose(0)}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h2>Remove {parts.length ? parts.join(", ") : `${ids.length} selected`}?</h2>

        {preview && preview.released > 0 && (
          <p className="hint">
            {preview.released} item{preview.released === 1 ? "" : "s"} gathered by{" "}
            {preview.collectors === 1 ? "that collector" : "those collectors"} will stay in the
            library — removing a collector removes the gathering, not what was gathered.
          </p>
        )}

        <label className="dialog-choice">
          <input
            type="checkbox"
            checked={trashFiles}
            disabled={!preview?.withFiles}
            onChange={(e) => setTrashFiles(e.target.checked)}
          />
          <span>
            <strong>Also move the files to Archiva's trash</strong>
            <span className="hint">
              {preview?.withFiles
                ? `${preview.withFiles} file${preview.withFiles === 1 ? "" : "s"} on disk. ` +
                  `They move into Archiva's own trash folder — out of every watched folder, and ` +
                  `still recoverable.`
                : "Nothing selected has a file on disk to move."}
            </span>
          </span>
        </label>

        {!trashFiles && preview?.withFiles ? (
          <p className="dialog-warn">
            The files stay where they are, so anything still inside a watched folder will be
            indexed again on the next scan — as a new item, without its tags.
          </p>
        ) : null}

        {error && <p className="dialog-warn error">{error}</p>}

        <div className="dialog-actions">
          <button className="btn" onClick={() => onClose(0)} disabled={busy}>
            Cancel
          </button>
          <button className="btn primary" onClick={confirm} disabled={busy || !preview}>
            {busy ? "Removing…" : trashFiles ? "Remove and trash files" : "Remove from library"}
          </button>
        </div>
        <p className="hint">This cannot be undone from inside Archiva.</p>
      </div>
    </div>
  );
}
