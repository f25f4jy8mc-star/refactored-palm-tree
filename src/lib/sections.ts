// The headers a list is divided into, and which of them are folded away.
//
// `p_rows` already decides what group each row belongs to and returns them in
// group order — the view's job is only to draw the headers and to remember
// which are shut. Recomputing the grouping here would be a second opinion
// about it, which is the bug class this rebuild exists to remove.
//
// A folded section keeps its header. Hiding the header too would make the
// rows look deleted, and there would be nothing left to click to get them
// back.

export type Grouped = { group_key: string; group_label: string };

export type Section = {
  key: string;
  label: string;
  /** How many rows it holds, whether or not they are currently drawn. */
  count: number;
};

/** The sections present, in the order the rows arrived in. */
export function sectionsOf<T extends Grouped>(rows: T[]): Section[] {
  const out: Section[] = [];
  for (const row of rows) {
    const last = out[out.length - 1];
    if (last && last.key === row.group_key) {
      last.count += 1;
      continue;
    }
    // A group the rows return to after another one is still one section:
    // p_rows sorts by group, so this only happens if something upstream
    // changes, and two headers with one name would be worse than a wrong
    // count.
    const existing = out.find((s) => s.key === row.group_key);
    if (existing) {
      existing.count += 1;
      continue;
    }
    out.push({ key: row.group_key, label: row.group_label, count: 1 });
  }
  return out;
}

/** The rows still drawn, given which sections are folded. */
export function visibleRows<T extends Grouped>(rows: T[], collapsed: string[]): T[] {
  if (collapsed.length === 0) return rows;
  return rows.filter((r) => !collapsed.includes(r.group_key));
}

export function toggleSection(collapsed: string[], key: string): string[] {
  return collapsed.includes(key) ? collapsed.filter((k) => k !== key) : [...collapsed, key];
}

export function isCollapsed(collapsed: string[], key: string): boolean {
  return collapsed.includes(key);
}
