// Mirrors `model::projections` on the Rust side, field for field. There is
// no independent frontend model — a view reads what the projection sends and
// nothing else, so a type drifting from the backend is a compile error here
// rather than a silent mismatch discovered at runtime.

export type NodeType = "media" | "note" | "collector" | "tag";

export type Availability =
  | "present"
  | "missing"
  | "remote_uncached"
  | "permission_denied";

export type ProxyState = "not_applicable" | "pending" | "ready" | "failed";

/** One row of `p_rows`. Already grouped, sorted and flattened server-side. */
export interface ListRow {
  id: string; // UUID v7
  node_type: NodeType;
  content_type: string;
  display_name: string;
  display_subtitle: string;
  icon_kind: string;
  availability: Availability;
  proxy_state: ProxyState;
  thumb_ref: string | null;
  size_bytes: number | null;
  indexed_at: string;
  captured_at: string | null;
  health: number;
  health_missing: string[];
  capabilities: string[];
  group_key: string;
  group_label: string;
  depth: number;
  ordinal: number;
  child_count: number;
}

export interface ListPage {
  rows: ListRow[];
  total: number;
  group_by: string;
  sort: string;
}

export type GroupBy = "type" | "health" | "month" | "none";
export type SortBy = "name" | "date" | "captured" | "size" | "health";

export interface ListOptions {
  scope: string | null;
  groupBy: GroupBy;
  sort: SortBy;
  descending: boolean;
  expanded: string[];
  query: string | null;
}

/** `p_detail`'s node shape — also what a search `Hit` carries (§3, `Row`). */
export interface Row {
  id: string;
  node_type: NodeType;
  content_type: string;
  display_name: string;
  display_subtitle: string;
  icon_kind: string;
  availability: Availability;
  proxy_state: ProxyState;
  thumb_ref: string | null;
  capabilities: string[];
}

export type MatchKind = "name" | "body" | "via_tag";

/** One `p_search` result. Sectioned by `match_kind` server-side already —
 * a name hit always precedes a body hit in the array the backend returns. */
export interface Hit {
  node: Row;
  match_kind: MatchKind;
  snippet: string;
}

/** One row of `view_prefs` (§1.9, G13) — a scope's remembered layout. */
export interface ViewPrefs {
  layout: string | null;
  sort: string | null;
  group_by: string | null;
  density: string | null;
  /** Unused since the Library became one flat listing. The column stays
   * because it is written, not because anything reads it. */
  shape: string | null;
}

/** One column of `p_tree` — the previous column's selected row, expanded.
 * Rows are the compact `Row` shape (same as p_detail/p_search), not
 * ListRow — a column shows icon, name and capabilities, not group/health
 * fields that only apply to a flat list. */
export interface TreeColumn {
  scope_id: string | null;
  title: string;
  rows: Row[];
}

/** One Space: a folder you chose, holding this library's index, its proxies
 * and the notes written here, plus the list of folders linked into it.
 * `reachable` is false when the folder is not there right now — a drive that
 * is not plugged in is an ordinary state, not a lost library. */
export interface Space {
  id: string;
  name: string;
  root: string;
  createdAt: string;
  lastOpenedAt: string | null;
  reachable: boolean;
  isCurrent: boolean;
  /** Bytes, by what made them: `index`, `proxies`, `notes`. */
  holds: Record<string, number>;
}

/** One watched folder (migration 002). `item_count` is derived from the
 * nodes underneath it, never stored. */
export interface Source {
  id: string;
  path: string;
  /** What is in effect — what the library is currently drawn from. */
  enabled: boolean;
  /** What the tickbox says, when that differs from what is in effect.
   * `null` means the two agree; Refresh is what closes the gap. */
  pending_enabled: boolean | null;
  added_at: string;
  last_scan_at: string | null;
  item_count: number;
}

/** What a batch of links came to (`relate::Added`). A refusal is reported
 * beside the rest — `[id, reason]` — rather than failing the whole gesture. */
export interface Added {
  created: number;
  existed: number;
  refused: [string, string][];
}

/** A collector made in Archiva, which is the only kind that takes members —
 * a folder mirrored from disk is not one (`relate::gather_target`). */
export interface GatherTarget {
  id: string;
  name: string;
  kind: "folder" | "board";
}

/** How many of a selection carry each tag and sit in each collector you
 * made, keyed by id. The tag popup's "every" and "some" come from this. */
export interface SelectionTags {
  total: number;
  tags: Record<string, number>;
  collectors: Record<string, number>;
  targets: GatherTarget[];
}

export type NewKind = "note" | "folder" | "board" | "link";

/** A space's own preferences — decisions about the whole library, true
 * wherever you look at it (not `ViewPrefs`, which is per pane, per scope). */
export interface Settings {
  showLinkedFolders: boolean;
}

/** One entry of a compass slot in `p_detail` — carries the far node's row
 * (G22), so drawing a compass is one call rather than one call per tile. */
export interface Link {
  edge_id: string;
  kind: string;
  compass: string;
  reciprocal: string;
  ordinal: number;
  label: string | null;
  status: string;
  origin: string;
  outward: boolean;
  node: Row;
}

export interface LinkGroup {
  node_type: string;
  total: number;
  links: Link[];
}

export interface Slot {
  compass: string;
  total: number;
  groups: LinkGroup[];
}

/** `p_detail`, plus the two fields the preview needs that `Row` lacks. */
export interface Detail {
  node: Row;
  attributes: Record<string, string>;
  slots: Slot[];
  suggestions: Link[];
  unresolved_links: number;
  locator: string | null;
  previewRef: string | null;
  sizeBytes: number | null;
}

/** A note's text, from the file on disk or from the `note` table (§1.3). */
export interface NoteBody {
  text: string;
  truncated: boolean;
  storage: "file" | "inline";
}

export interface ScanReport {
  seen: number;
  created: number;
  updated: number;
  touched: number;
  refreshed: number;
  deferred: number;
  unreadable: number;
  wentMissing: number;
}

/* ------------------------------------------------- classification (C1) */

/** One facet, served from the backend rather than restated here. The
 * facet→tier pair is already denormalised once in the database; a third copy
 * in TypeScript is how three copies drift. */
export interface Facet {
  id: string;
  label: string;
  tier: number;
  tier_label: string;
  hint: string;
  machine_fillable: boolean;
}

export interface Tag {
  id: string;
  name: string;
  facet: string;
  tier: number;
  sort_order: number;
  /** How many items carry it. Derived from the edges, never stored. */
  usage: number;
}

/** Two tags in one facet that differ by a character or a plural (C4). */
export interface DuplicatePair {
  key: string;
  a: Tag;
  b: Tag;
  reason: string;
}

/** A tag proposed for one item, never applied (C5, principle 3). Two rungs
 * of the same tier-one ladder: `metadata_tag` is read off the file's own
 * metadata, `vocabulary_tag` is a tag you already coined that this item
 * looks like it wants. `kind` is what dismissing it is keyed by. */
export interface TagSuggestion {
  key: string;
  kind: "metadata_tag" | "vocabulary_tag";
  facet: string;
  name: string;
  evidence: string;
}

/* ------------------------------------------------------ p_record (I/C) */

export interface Identity {
  id: string;
  nodeType: NodeType;
  contentType: string;
  /** The materialised conformance closure, leaf first. */
  conformsTo: string[];
  title: string;
  displayName: string;
  displaySubtitle: string;
  iconKind: string;
  createdAt: string;
  indexedAt: string;
  modifiedAt: string;
}

export type SourceKind = "local_file" | "remote_url" | "app_generated";

export interface SourceFacts {
  sourceKind: SourceKind;
  locator: string | null;
  parentDir: string | null;
  filename: string | null;
  extension: string | null;
  sizeBytes: number | null;
  /** BLAKE3 — an attribute beside the identity, never the identity. */
  contentHash: string | null;
  inode: number | null;
  device: number | null;
  mtime: string | null;
  ctime: string | null;
  availability: Availability;
  lastSeenAt: string | null;
}

/** Four artefacts tracked separately (G3), not one thumbnail field. */
export interface ProxySet {
  thumbRef: string | null;
  previewRef: string | null;
  playableRef: string | null;
  originalAvailable: boolean;
  version: number;
  state: ProxyState;
}

export interface FacetSlot {
  facet: string;
  label: string;
  hint: string;
  tier: number;
  machineFillable: boolean;
  tags: Tag[];
}

export interface TierBlock {
  tier: number;
  label: string;
  facets: FacetSlot[];
}

export interface Classification {
  tiers: TierBlock[];
  suggestions: TagSuggestion[];
}

export interface HealthBlock {
  score: number;
  label: string;
  description: string;
  facetsFilled: number;
  facetTarget: number;
  titleQuality: number;
  hasAnyTag: number;
  unresolvedLinks: number;
}

/** One decision the reconciler recorded about this item. */
export interface IndexEvent {
  at: string;
  rule: number;
  ruleLabel: string;
  ruleNote: string;
  signals: string[];
  tableVersion: number;
  locator: string | null;
}

/** `p_record` — everything known about one item, in one call. Wraps p_detail
 * rather than duplicating it, so the Inspector still reads one projection. */
export interface ItemRecord extends Detail {
  identity: Identity;
  source: SourceFacts;
  proxies: ProxySet;
  classification: Classification;
  health: HealthBlock;
  history: IndexEvent[];
}

export interface Recheck {
  present: number;
  missing: number;
  permission_denied: number;
  remote_uncached: number;
}

/* ---------------------------------------------------------- removal */

/** What is about to go, asked for before anything goes — there is no undo. */
export interface RemovalPreview {
  items: number;
  collectors: number;
  notes: number;
  /** Members that will be released from the collectors being removed. They
   * stay in the library; deleting a folder never deletes what it gathered. */
  released: number;
  withFiles: number;
}

export interface RemovalResult {
  forgotten: number;
  trashed: number;
  /** Files that would not move. Their rows are kept, so a partial failure
   * never reads as a success. */
  failed: string[];
}
