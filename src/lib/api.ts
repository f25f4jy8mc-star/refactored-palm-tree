import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type {
  Detail,
  DuplicatePair,
  Facet,
  Hit,
  ItemRecord,
  ListOptions,
  ListPage,
  Recheck,
  ScanReport,
  Source,
  Tag,
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

/* --------------------------------------------------------- p_record */

/** Everything known about one item. The Inspector's single read. */
export function nodeRecord(id: string): Promise<ItemRecord> {
  return invoke("node_record", { id });
}

/* -------------------------------------------------- classification */

export function listFacets(): Promise<Facet[]> {
  return invoke("list_facets");
}

export function listTags(): Promise<Tag[]> {
  return invoke("list_tags");
}

export function createTag(name: string, facet: string): Promise<string> {
  return invoke("create_tag", { name, facet });
}

/** Applying and removing take a list, always — batch is the default shape,
 * because tagging forty things one at a time is how a library dies. */
export function applyTag(nodeIds: string[], tagId: string): Promise<number> {
  return invoke("apply_tag", { nodeIds, tagId });
}

export function removeTag(nodeIds: string[], tagId: string): Promise<number> {
  return invoke("remove_tag", { nodeIds, tagId });
}

export function renameTag(tagId: string, name: string): Promise<void> {
  return invoke("rename_tag", { tagId, name });
}

export function setTagFacet(tagId: string, facet: string): Promise<void> {
  return invoke("set_tag_facet", { tagId, facet });
}

export function deleteTag(tagId: string): Promise<number> {
  return invoke("delete_tag", { tagId });
}

export function mergeTags(from: string, into: string): Promise<number> {
  return invoke("merge_tags", { from, into });
}

export function reorderTag(tagId: string, to: number): Promise<void> {
  return invoke("reorder_tag", { tagId, to });
}

export function promoteTag(
  tagId: string,
  name: string | null,
  stripTag: boolean,
): Promise<{ collectorId: string; moved: number }> {
  return invoke("promote_tag", { tagId, name, stripTag });
}

export function duplicateTags(): Promise<DuplicatePair[]> {
  return invoke("duplicate_tags");
}

export function acceptSuggestion(
  nodeId: string,
  facet: string,
  name: string,
): Promise<string> {
  return invoke("accept_suggestion", { nodeId, facet, name });
}

export function dismissSuggestion(key: string, kind: string): Promise<void> {
  return invoke("dismiss_suggestion", { key, kind });
}

/* ------------------------------------------------- source and reach */

export function addRemoteItem(url: string, title: string | null): Promise<string> {
  return invoke("add_remote_item", { url, title });
}

/** Re-examine everything not currently present, without a full walk. */
export function recheckAvailability(): Promise<Recheck> {
  return invoke("recheck_availability");
}
