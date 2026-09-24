// Where the highlight is in a Miller cascade, and where it goes next.
//
// The cascade is a path of opened folders plus one more column after it.
// Every column along the path has its highlight on the folder it opened; the
// last column's highlight is a separate cursor. That split is the whole state.
//
// Pulled out because the rule broke in a way no one could see from the
// component: after ← back out of a folder, ↓ onto a file closed the columns
// to its right, the column you were in became the last one, and its cursor
// — still pointing into the column that had just closed — was reset to the
// top. The highlight jumped to the first row and the next ↓ landed where the
// previous one should have. The fix is one line here: selecting something
// that does not open makes *it* the cursor of the column it is in.

export type Cascade = {
  /** The folders opened, one per column, left to right. */
  path: string[];
  /** The highlight in the column after the path. */
  cursor: string | null;
};

/** The id highlighted in column `col`, or null. */
export function highlighted(c: Cascade, col: number): string | null {
  if (col < c.path.length) return c.path[col];
  if (col === c.path.length) return c.cursor;
  return null;
}

/** The cascade after selecting `id` in column `col`.
 *
 * A folder opens: the path runs to it, and the column it opens starts with no
 * highlight of its own (the view puts one on its first row when it arrives).
 * Anything else closes the columns to its right and becomes the highlight in
 * the column it is in — which is now the last one. */
export function select(c: Cascade, col: number, id: string, opens: boolean): Cascade {
  if (opens) {
    if (c.path[col] === id && c.path.length === col + 1) return c;
    return { path: [...c.path.slice(0, col), id], cursor: null };
  }
  return { path: c.path.slice(0, col), cursor: id };
}

/** The id one step up or down from the highlight in a column's order. Stops
 * at the ends rather than wrapping, as a Finder column does. With no
 * highlight, the first step lands on the first row. */
export function step(order: string[], from: string | null, delta: 1 | -1): string | null {
  if (order.length === 0) return null;
  const i = from ? order.indexOf(from) : -1;
  if (i === -1) return order[0];
  return order[Math.min(order.length - 1, Math.max(0, i + delta))];
}
