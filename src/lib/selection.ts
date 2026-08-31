// Finder's selection rules. The one detail worth stating up front, because
// getting it wrong is a real, specific, previously-shipped bug: **the anchor
// and the cursor are different things.** The anchor is fixed at the start of
// a range gesture (⇧-click, ⇧-arrow) and the cursor is wherever the range
// currently ends. Conflating them means every ⇧-arrow press resets the
// anchor to where the cursor just was, so the range can never grow past one
// row — pressing ⇧-down five times selects two rows, not six.
//
// Pure and stateless: every operation takes the current state and the
// visible order (the same ordinal sequence a projection returns) and returns
// the next state. No view owns a private copy of this.

export interface SelectionState {
  ids: ReadonlySet<string>;
  anchor: string | null;
  cursor: string | null;
}

export const EMPTY_SELECTION: SelectionState = {
  ids: new Set(),
  anchor: null,
  cursor: null,
};

function rangeBetween(order: readonly string[], a: string, b: string): string[] {
  const ia = order.indexOf(a);
  const ib = order.indexOf(b);
  if (ia === -1 || ib === -1) return [b];
  const [lo, hi] = ia <= ib ? [ia, ib] : [ib, ia];
  return order.slice(lo, hi + 1);
}

/** Plain click: replaces the selection with exactly this row. */
export function click(id: string): SelectionState {
  return { ids: new Set([id]), anchor: id, cursor: id };
}

/** ⌘/Ctrl-click: toggles one row in or out. The anchor moves to it either
 * way, so a following ⇧-click ranges from here, not from the old anchor. */
export function toggleClick(state: SelectionState, id: string): SelectionState {
  const ids = new Set(state.ids);
  if (ids.has(id)) {
    ids.delete(id);
  } else {
    ids.add(id);
  }
  return { ids, anchor: id, cursor: id };
}

/** ⇧-click: replaces the selection with the contiguous range from the fixed
 * anchor to `id`. Repeated ⇧-clicks keep growing or shrinking from the same
 * anchor rather than chaining from the last click. */
export function rangeClick(
  state: SelectionState,
  id: string,
  order: readonly string[],
): SelectionState {
  const anchor = state.anchor ?? id;
  return { ids: new Set(rangeBetween(order, anchor, id)), anchor, cursor: id };
}

export function selectAll(order: readonly string[]): SelectionState {
  return {
    ids: new Set(order),
    anchor: order[0] ?? null,
    cursor: order[order.length - 1] ?? null,
  };
}

export function clear(): SelectionState {
  return EMPTY_SELECTION;
}

/**
 * Arrow-key movement. `extend` is the shift modifier: with it, the anchor
 * holds and the range grows or shrinks to the new cursor position — the
 * fix for the bug described above. Without it, this is a plain move that
 * replaces the selection, same as clicking the row landed on.
 */
export function moveCursor(
  state: SelectionState,
  order: readonly string[],
  delta: number,
  extend: boolean,
): SelectionState {
  if (order.length === 0) return state;
  const from = state.cursor && order.includes(state.cursor) ? order.indexOf(state.cursor) : -1;
  const next = Math.min(Math.max(from + delta, 0), order.length - 1);
  const id = order[next];
  if (!extend) return click(id);
  const anchor = state.anchor ?? id;
  return { ids: new Set(rangeBetween(order, anchor, id)), anchor, cursor: id };
}

/**
 * Marquee/rubber-band drag: `hit` is whatever currently sits inside the drag
 * rectangle. Additive (⇧ or ⌘ held at drag start) unions with the selection
 * that existed before the drag began; otherwise the rectangle replaces it.
 */
export function marquee(
  base: SelectionState,
  hit: readonly string[],
  additive: boolean,
): SelectionState {
  const ids = additive ? new Set([...base.ids, ...hit]) : new Set(hit);
  const cursor = hit[hit.length - 1] ?? base.cursor;
  return { ids, anchor: base.anchor ?? cursor ?? null, cursor: cursor ?? null };
}

export function isSelected(state: SelectionState, id: string): boolean {
  return state.ids.has(id);
}

/** Type-ahead: jump to the next row whose name starts with the typed
 * prefix, wrapping past the end and starting just after the current cursor
 * so repeated presses of the same letter cycle through matches. */
export function typeAhead(
  order: readonly string[],
  names: ReadonlyMap<string, string>,
  prefix: string,
  cursor: string | null,
): string | null {
  const lower = prefix.toLowerCase();
  const from = cursor ? Math.max(order.indexOf(cursor), 0) : -1;
  for (let i = 1; i <= order.length; i++) {
    const id = order[(from + i) % order.length];
    if ((names.get(id) ?? "").toLowerCase().startsWith(lower)) return id;
  }
  return null;
}
