// Quick Look. Space opens it on whatever is focused, Space or Escape
// closes it, I toggles the info panel and F fills the pane.
//
// It shows what the view behind it has selected, and nothing else. Stepping
// through items is that view's job: ←/→ pass straight through to it, its
// cursor moves, and this overlay follows the active item. Giving the preview
// its own next-and-previous meant two ideas of "the current item", and the
// list and the preview drifted apart the moment either one was touched.
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
  // Reads the active item rather than a frozen id, so when the list behind
  // moves its cursor, this overlay and the Inspector move with it. One idea
  // of "the current item" — the seam the old build's inspector/preview
  // desync lived in.
  const { id, step, setActive } = useActiveItem();

  useEffect(() => {
    if (!id) return;
    setFailed(false);
    nodeDetail(id).then(setDetail).catch(() => setDetail(null));
  }, [id]);

  // The bar's ‹ and › move the same active item the list publishes, so a
  // click and an arrow key end up in the same place. They are a convenience
  // for the mouse, not a second navigation model.
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
      // Arrows are deliberately *not* consumed. The overlay shows whatever
      // the view behind it has selected, so moving through items is the
      // view's job — its cursor moves, it publishes the new active item, and
      // this overlay follows. Handling them here as well gave the preview a
      // second idea of "next", which is how it and the list ended up on
      // different items.
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
