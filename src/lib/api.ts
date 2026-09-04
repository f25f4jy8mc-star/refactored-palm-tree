import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type {
  Detail,
  Hit,
  ListOptions,
  ListPage,
  ScanReport,
  Source,
  TreeColumn,
  ViewPrefs,
} from "./types";

export function listRows(opts: ListOptions): Promise<ListPage> {
  return invoke("list_rows", {
    args: {
      scope: opts.scope,
      groupBy: opts.groupBy,
      sort: opts.sort,
      descending: opts.descending,
      expanded: opts.expanded,
      query: opts.query,
    },
  });
}

export function searchLibrary(query: string, typeFilter?: string | null): Promise<Hit[]> {
  return invoke("search_library", {
    args: { query, typeFilter: typeFilter ?? null, limit: 50 },
  });
}

export function treeColumns(path: string[]): Promise<TreeColumn[]> {
  return invoke("tree_columns", { path });
}

export function nodeDetail(id: string): Promise<Detail> {
  return invoke("node_detail", { id });
}

/* ------------------------------------------------------------- sources */

export function listSources(): Promise<Source[]> {
  return invoke("list_sources");
}

export function addSource(path: string): Promise<ScanReport> {
  return invoke("add_source", { path });
}

export function removeSource(id: string): Promise<void> {
  return invoke("remove_source", { id });
}

export function setSourceEnabled(id: string, enabled: boolean): Promise<void> {
  return invoke("set_source_enabled", { id, enabled });
}

/** Re-index every enabled source in one pass — there is deliberately no
 * scan-one-folder call, see `commands::rescan`. */
export function rescan(): Promise<ScanReport> {
  return invoke("rescan");
}

/* --------------------------------------------------------- view prefs */

export function getViewPrefs(scopeId: string, paneKind: string): Promise<ViewPrefs> {
  return invoke("get_view_prefs", { scopeId, paneKind });
}

export function setViewPrefs(scopeId: string, paneKind: string, prefs: ViewPrefs): Promise<void> {
  return invoke("set_view_prefs", { scopeId, paneKind, prefs });
}

/** Native folder picker. Returns null when the user cancels. */
export async function pickFolder(): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}
