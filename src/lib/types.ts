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
