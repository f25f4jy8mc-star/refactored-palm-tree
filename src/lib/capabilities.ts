// The capability registry, frontend half.
//
// Per §2.4 of the model (G15), a capability is type grant AND instance
// predicate, and both halves need database state the projection already has
// — availability, proxy readiness, item count. So resolution happens exactly
// once, server-side in `model::capabilities`, and every row that comes back
// from `p_rows` already carries its resolved `capabilities: string[]`.
//
// This file does not re-derive that list — a second resolver here would be a
// second copy of the registry, and two copies is the exact failure mode this
// rebuild exists to remove. What it owns is what a *view* does with an
// already-resolved list: which single capability wins a double-click, and
// what a button is called. Both are presentation, not resolution.

import type { ListRow } from "./types";

export type Capability =
  | "preview"
  | "full_res"
  | "play"
  | "seek"
  | "queue"
  | "paginate"
  | "orbit"
  | "edit"
  | "embed"
  | "expand"
  | "contain"
  | "position"
  | "export"
  | "tag"
  | "link"
  | "rename"
  | "delete"
  | "reveal"
  | "fetch"
  | "promote"
  | "set_facet";

export function can(row: ListRow, capability: Capability): boolean {
  return row.capabilities.includes(capability);
}

export type OpenTarget =
  | "fetch"
  | "expand"
  | "edit"
  | "play"
  | "paginate"
  | "orbit"
  | "preview";

/**
 * The single ordered rule for what a double-click does — the same order as
 * `model::capabilities::open_target`. Most particular renderer wins, so a
 * playable PDF (there is no such thing today, but the rule should not need
 * to change if one exists tomorrow) would still paginate before it previews.
 */
const OPEN_PRIORITY: OpenTarget[] = [
  "fetch",
  "expand",
  "edit",
  "play",
  "paginate",
  "orbit",
  "preview",
];

export function openTarget(row: ListRow): OpenTarget | null {
  for (const target of OPEN_PRIORITY) {
    if (can(row, target)) return target;
  }
  return null;
}

/** Row-action labels, in the order §2.3 of the model lists them. */
export const CAPABILITY_LABEL: Record<Capability, string> = {
  preview: "Preview",
  full_res: "Open full resolution",
  play: "Play",
  seek: "Seek",
  queue: "Queue",
  paginate: "Open",
  orbit: "Orbit",
  edit: "Edit",
  embed: "Embed",
  expand: "Expand",
  contain: "Drop into",
  position: "Position",
  export: "Export",
  tag: "Tag…",
  link: "Link…",
  rename: "Rename",
  delete: "Delete",
  reveal: "Reveal in Finder",
  fetch: "Fetch",
  promote: "Promote",
  set_facet: "Set facet…",
};
