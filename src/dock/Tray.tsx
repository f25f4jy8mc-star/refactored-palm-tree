// The staging tray, at the end of the taskbar (Build 17's, carried across).
//
// Gather from anywhere — drag rows onto it, or "⊕ Tray" on a selection — then
// put the lot somewhere in one gesture: into the collector a Viewer is showing,
// or into one arm of the item the Inspector is showing. The bar offers those
// as actions; the tray itself holds, shows and lets go.
//
// Its entries are drawn from `rowsOf`, not from anything carried in, so a
// renamed item shows its new name and a removed one drops out on the next
// change event. Each entry can be dragged back out, onto a compass arm.

import { useCallback, useEffect, useState } from "react";

import { rowsOf } from "../lib/api";
import { droppedIds, isItemDrag, itemDrag } from "../lib/drag";
import { useArchivaChanged } from "../lib/events";
import type { Row } from "../lib/types";
import { useWorkbench } from "../lib/workbench";
import { Thumbnail } from "../components/library/Thumbnail";

/** Enough to recognise what is there; the count says the rest. */
const SHOWN = 6;

export function Tray({ onOpen }: { onOpen: (id: string) => void }) {
  const { tray, addToTray, removeFromTray, clearTray, dropMissing } = useWorkbench();
  const [rows, setRows] = useState<Row[]>([]);
  const [over, setOver] = useState(false);

  const load = useCallback(async () => {
    if (tray.length === 0) {
      setRows([]);
      return;
    }
    const asked = tray;
    try {
      const got = await rowsOf(asked);
      setRows(got);
      // Something removed from the library while it sat here is gone from
      // here too — a tray entry for nothing would put nothing somewhere.
      dropMissing(asked, got.map((r) => r.id));
    } catch {
      /* the strip keeps what it last drew; the next change event retries */
    }
  }, [tray, dropMissing]);

  useEffect(() => {
    load();
  }, [load]);
  useArchivaChanged(load);

  const drop = {
    onDragOver: (e: React.DragEvent) => {
      if (!isItemDrag(e)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
      setOver(true);
    },
    onDragLeave: () => setOver(false),
    onDrop: (e: React.DragEvent) => {
      setOver(false);
      const ids = droppedIds(e);
      if (ids.length === 0) return;
      e.preventDefault();
      addToTray(ids);
    },
  };

  if (tray.length === 0) {
    return (
      <span
        className={"tray tray-empty" + (over ? " over" : "")}
        title="The tray — drag items here, then put them somewhere in one go"
        {...drop}
      >
        ⊕ tray
      </span>
    );
  }

  // Only what the backend answered for is drawn; until it answers, the
  // count stands in, so a slow reply never shows an empty strip that says 3.
  const shown = rows.slice(0, SHOWN);
  return (
    <div
      className={"tray" + (over ? " over" : "")}
      title="The tray — put what it holds somewhere from the bar, or drag an entry onto a compass arm"
      {...drop}
    >
      {shown.map((r) => (
        <span
          key={r.id}
          className="tray-item"
          title={`${r.display_name} — drag onto a compass arm, click to inspect`}
          {...itemDrag(() => [r.id])}
          onClick={() => onOpen(r.id)}
        >
          <Thumbnail item={r} />
          <button
            className="tray-x"
            title={`Take ${r.display_name} out of the tray`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={(e) => {
              e.stopPropagation();
              removeFromTray(r.id);
            }}
          >
            ×
          </button>
        </span>
      ))}
      {tray.length > SHOWN && <span className="tray-more">+{tray.length - SHOWN}</span>}
      <span className="tray-count">{tray.length}</span>
      <button
        className="btn quiet tray-drag"
        title="Drag the whole tray onto a compass arm"
        {...itemDrag(() => tray)}
      >
        ⠿
      </button>
      <button
        className="btn quiet"
        title="Empty the tray — nothing in the library changes"
        onMouseDown={(e) => e.preventDefault()}
        onClick={clearTray}
      >
        Clear
      </button>
    </div>
  );
}
