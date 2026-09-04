import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { Hit, ListOptions, ListPage, ScanReport, TreeColumn, ViewPrefs } from "./types";

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

export function scanFolder(path: string): Promise<ScanReport> {
  return invoke("scan_folder", { path });
}

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
