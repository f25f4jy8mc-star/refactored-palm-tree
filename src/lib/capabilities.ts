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

/** What both `ListRow` and a search `Hit`'s `Row` carry — everything below
 * needs nothing else, so it takes this instead of committing to one shape. */
type HasCapabilities = { capabilities: string[] };

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

export function can(row: HasCapabilities, capability: Capability): boolean {
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

export function openTarget(row: HasCapabilities): OpenTarget | null {
  for (const target of OPEN_PRIORITY) {
    if (can(row, target)) return target;
  }
  return null;
}

/* --------------------------------------------------------- what to draw */

/** The three references an item can be drawn from, plus its resolved list. */
export type PreviewParts = {
  capabilities: string[];
  locator: string | null;
  previewRef: string | null;
  thumbRef: string | null;
};

/**
 * The best image to draw for an item, or null when there is none.
 *
 * `full_res` and `preview` already encode "is the original reachable" versus
 * "is there a proxy" — both halves resolved server-side — so this reads them
 * rather than asking again. The order is: the original when the registry says
 * it can be opened at full resolution, then the preview-sized render, then
 * the grid thumbnail, then the original as a last resort (a file that is
 * there but has no proxy yet).
 *
 * One copy, two callers: Quick Look and the Inspector's expanded preview. Two
 * copies would be two answers to "what does this item look like".
 */
export function previewSource(parts: PreviewParts): string | null {
  const { capabilities, locator, previewRef, thumbRef } = parts;
  if (capabilities.includes("full_res") && locator) return locator;
  return previewRef ?? thumbRef ?? locator;
}

/* ------------------------------------------------------ how to render it */

export type PreviewKind =
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "text"
  | "model"
  | "none";

/**
 * Which renderer draws this item, from what the registry already resolved.
 *
 * `openTarget` is the same ordered rule a double-click uses, so the two can
 * never disagree about what an item *is*; this only turns that answer into a
 * component. Audio and video both resolve to `play` — the registry has one
 * grant for audiovisual content — so the split between them is `icon_kind`,
 * which `content_type::icon_kind` derived from the conformance closure
 * server-side. No extension is compared anywhere (G17).
 *
 * `none` is an honest answer, not a failure: a collector has nothing to draw,
 * and a file that is missing has had `preview` withheld.
 */
export function previewKind(row: HasCapabilities & { icon_kind: string }): PreviewKind {
  switch (openTarget(row)) {
    case "play":
      return row.icon_kind === "audio" ? "audio" : "video";
    case "paginate":
      return "pdf";
    case "edit":
      return "text";
    case "orbit":
      return "model";
    case "preview":
      return row.icon_kind === "image" ? "image" : "none";
    default:
      // `fetch` (not here yet) and `expand` (a collector) have no still to
      // show, and neither does an item the registry granted nothing for.
      return "none";
  }
}

/* ------------------------------------------------------------- open in… */

export type Destination = "viewer" | "library" | "graph";

export type OpenOption = {
  destination: Destination;
  label: string;
  /** Why it is offered, or why it is offered and inert. */
  note: string;
  /** False for a destination that applies but has no view yet. */
  enabled: boolean;
};

/**
 * Where an item can be opened, decided from its resolved capabilities rather
 * than from its type spelled out here — a HEIC is an image without anyone
 * editing a list (G17), and the same holds for this.
 *
 * A destination that does not apply is left out; one that applies but is not
 * built is offered and disabled, saying so. Hiding an unbuilt view would make
 * "whatever is applicable" quietly mean "whatever we finished", and the rail
 * already shows unbuilt destinations the same way.
 */
export function openDestinations(row: HasCapabilities & { node_type: string }): OpenOption[] {
  const out: OpenOption[] = [];
  // Only a collector opens *as* a viewer: the Viewer shows what something
  // contains, and a photograph contains nothing. `expand` is exactly that
  // grant, so a board — which has no contents to cascade — is not offered
  // one either.
  if (can(row, "expand")) {
    out.push({
      destination: "viewer",
      label: "Viewer",
      note: "its contents, as columns",
      enabled: true,
    });
  }
  // The Library lists everything except the vocabulary itself.
  if (row.node_type !== "tag") {
    out.push({
      destination: "library",
      label: "Library",
      note: "find it in the listing",
      enabled: true,
    });
  }
  if (can(row, "link")) {
    out.push({
      destination: "graph",
      label: "Graph",
      note: "not built yet",
      enabled: false,
    });
  }
  return out;
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
