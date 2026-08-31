import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { ListOptions, ListPage, ScanReport } from "./types";

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

export function scanFolder(path: string): Promise<ScanReport> {
  return invoke("scan_folder", { path });
}

/** Native folder picker. Returns null when the user cancels. */
export async function pickFolder(): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}
