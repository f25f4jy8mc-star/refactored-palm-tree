// Quick Look. Space opens it on whatever is focused, Space or Escape
// closes it, ←/→ step through the siblings of the list it was opened from,
// I toggles the info panel and F fills the pane — Build 17's bindings.
//
// The sibling order arrives live from `useActiveItem` rather than as a
// copied array (G16): a snapshot goes stale the moment the underlying list
// re-sorts or a scan adds a row, which is exactly how the old build's
// preview ended up walking an order the screen no longer showed.
//
// What it can draw is decided by capability, never by file extension: an
// image previews, a PDF or a video says plainly that its viewer isn't
// built rather than showing a broken frame.

import { useCallback, useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { nodeDetail } from "../../lib/api";
import { useActiveItem } from "../../lib/activeItem";
import { openTarget } from "../../lib/capabilities";
import type { Detail } from "../../lib/types";

function formatBytes(bytes: number | null): string | null {
  if (!bytes) return null;
  const mb = bytes / 1_048_576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

export function PreviewOverlay({ onClose }: { onClose: () => void }) {
  const [detail, setDetail] = useState<Detail | null>(null);
  const [info, setInfo] = useState(false);
  const [filled, setFilled] = useState(false);
  const [failed, setFailed] = useState(false);
  // Reads the active item rather than a frozen id: ←/→ move the active
  // item, and everything following it — this overlay and the Inspector
  // behind it — moves together. Two ideas of "the current item" is the
  // seam the old build's inspector/preview desync lived in.
  const { id, step, setActive } = useActiveItem();

  useEffect(() => {
    if (!id) return;
    setFailed(false);
    nodeDetail(id).then(setDetail).catch(() => setDetail(null));
  }, [id]);

  const navigate = useCallback(
    (delta: number) => {
      const next = step(delta);
      if (next) setActive(next);
    },
    [step, setActive],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // While the overlay is up it owns the keyboard: a key it acts on is
      // stopped here rather than left to run on down the path. Closing on
      // Space re-renders the shell synchronously — React flushes a discrete
      // event's state before the bubble phase — so a shell listener that
      // merely checked "is the preview open?" would re-register with the
      // fresh answer and reopen the overlay on the very keystroke that
      // closed it. Stopping propagation is what makes it modal.
      const consume = () => {
        e.preventDefault();
        e.stopPropagation();
        e.stopImmediatePropagation();
      };
      if (e.key === "Escape" || e.code === "Space") {
        consume();
        onClose();
        return;
      }
      if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        consume();
        navigate(e.key === "ArrowRight" ? 1 : -1);
        return;
      }
      if (e.key.toLowerCase() === "i") {
        consume();
        setInfo((v) => !v);
        return;
      }
      if (e.key.toLowerCase() === "f") {
        consume();
        setFilled((v) => !v);
      }
    };
    // Capture, so the overlay's keys win over whichever view is behind it.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, navigate]);

  if (!detail) return null;

  const { node } = detail;
  // Full resolution when the original is actually reachable, else the
  // proxy — which is exactly what `preview` vs `full_res` already encode.
  const canFullRes = node.capabilities.includes("full_res");
  const src =
    canFullRes && detail.locator
      ? detail.locator
      : detail.previewRef ?? node.thumb_ref ?? detail.locator;
  const isImage = node.icon_kind === "image";
  const target = openTarget(node);

  return (
    <div className="preview-backdrop" onClick={onClose}>
      <div
        className={"preview" + (filled ? " filled" : "")}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="preview-stage">
          {isImage && src && !failed ? (
            <img src={convertFileSrc(src)} alt="" onError={() => setFailed(true)} />
          ) : (
            <div className="preview-placeholder">
              <div className="preview-glyph">{node.display_name.slice(0, 1).toUpperCase()}</div>
              <div>
                {failed
                  ? "That file couldn't be loaded."
                  : target && target !== "preview"
                    ? `This opens via ${target} — that viewer isn't built yet.`
                    : "No preview available for this item."}
              </div>
            </div>
          )}
        </div>

        <div className="preview-bar">
          <button className="btn" onMouseDown={(e) => e.preventDefault()} onClick={() => navigate(-1)}>
            ‹
          </button>
          <span className="preview-name">{node.display_name}</span>
          <span className="preview-sub">{node.display_subtitle}</span>
          <span className="taskbar-spacer" />
          <button
            className={"btn" + (info ? " on" : "")}
            title="Info (I)"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setInfo((v) => !v)}
          >
            i
          </button>
          <button
            className={"btn" + (filled ? " on" : "")}
            title="Fill (F)"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setFilled((v) => !v)}
          >
            ⤢
          </button>
          <button className="btn" onMouseDown={(e) => e.preventDefault()} onClick={() => navigate(1)}>
            ›
          </button>
          <button className="btn" title="Close (Space)" onClick={onClose}>
            ✕
          </button>
        </div>

        {info && (
          <div className="preview-info">
            <dl>
              <dt>Type</dt>
              <dd className="mono">{node.content_type}</dd>
              {formatBytes(detail.sizeBytes) && (
                <>
                  <dt>Size</dt>
                  <dd>{formatBytes(detail.sizeBytes)}</dd>
                </>
              )}
              <dt>Availability</dt>
              <dd>{node.availability.replace("_", " ")}</dd>
              {Object.entries(detail.attributes)
                .slice(0, 8)
                .map(([k, v]) => (
                  <div key={k} style={{ display: "contents" }}>
                    <dt>{k.replace(/_/g, " ")}</dt>
                    <dd className="mono wrap">{v}</dd>
                  </div>
                ))}
            </dl>
          </div>
        )}
      </div>
    </div>
  );
}
