// Dragging items between surfaces: a list into the tray, the tray into a
// compass arm, a row straight onto an arm.
//
// One payload type, carrying ids and nothing else. What an id *is* — its
// name, its thumbnail, whether it can go where it is dropped — is asked of the
// backend at the drop, not carried in the drag, so a drag started before a
// rename cannot deliver a stale name, and a drop target never has to trust a
// description it was handed.

export const ITEM_MIME = "application/x-archiva-ids";

/** What a drag from `rowId` carries: the whole selection when the row is part
 * of it — dragging one of three highlighted rows moves all three, as it does
 * in every file manager — and otherwise just the row. */
export function dragIds(rowId: string, selected: readonly string[]): string[] {
  return selected.includes(rowId) ? [...selected] : [rowId];
}

/** Read a drop back. Anything that is not a well-formed list of ids is not
 * an item drag — a file from the desktop, a dockview tab, text — and gets an
 * empty answer rather than an exception in the middle of a drop. */
export function parseIds(raw: string | null | undefined): string[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    return Array.isArray(v) && v.every((x) => typeof x === "string" && x.length > 0)
      ? (v as string[])
      : [];
  } catch {
    return [];
  }
}

/** True while an item drag is over something. `getData` is not readable
 * until the drop, so a target decides whether to light up from the type. */
export function isItemDrag(e: { dataTransfer: DataTransfer | null }): boolean {
  return !!e.dataTransfer && Array.from(e.dataTransfer.types).includes(ITEM_MIME);
}

/** Spread onto a row: `<div {...itemDrag(() => ids)}>`. The ids are read
 * when the drag starts, not when the row renders, so the selection it
 * carries is the one on screen at that moment. */
export function itemDrag(ids: () => string[]) {
  return {
    draggable: true,
    onDragStart: (e: React.DragEvent) => {
      const payload = ids();
      e.dataTransfer.setData(ITEM_MIME, JSON.stringify(payload));
      // Plain text as well, so dropping into a text field says something
      // sensible rather than nothing.
      e.dataTransfer.setData("text/plain", `${payload.length} item${payload.length === 1 ? "" : "s"}`);
      e.dataTransfer.effectAllowed = "copyLink";
    },
  };
}

/** The ids a drop carried. */
export function droppedIds(e: React.DragEvent): string[] {
  return parseIds(e.dataTransfer.getData(ITEM_MIME));
}
