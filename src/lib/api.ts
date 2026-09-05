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
  RemovalPreview,
  RemovalResult,
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
      shape: opts.shape ?? "source",
    },
  });
}

export function searchLibrary(query: string, typeFilter?: string | null): Promise<Hit[]> {
  return invoke("search_library", {
    args: { query, typeFilter: typeFilter ?? null, limit: 50 },
  });
}

/** The Miller cascade. `root` is the collector the first column shows the
 * inside of; null is the library root. A folder nested inside another is not
 * in the library's root column, so a pane scoped to one has to start there. */
export function treeColumns(root: string | null, path: string[]): Promise<TreeColumn[]> {
  return invoke("tree_columns", { root, path });
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

/** Stop watching a folder. `forgetItems` false keeps everything indexed from
 * it — tags, links and notes are the user's work. True forgets those rows;
 * it never touches a file. */
export function removeSource(id: string, forgetItems = false): Promise<number> {
  return invoke("remove_source", { id, forgetItems });
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

/* ------------------------------------------------------------ removal */

export function previewRemoval(ids: string[]): Promise<RemovalPreview> {
  return invoke("preview_removal", { ids });
}

/** `trashFiles` false forgets the rows only, so a file still inside a watched
 * folder returns on the next scan as a new item with none of its tags. True
 * moves the file into Archiva's trash first, which is outside every watched
 * folder and still on disk. */
export function deleteItems(ids: string[], trashFiles: boolean): Promise<RemovalResult> {
  return invoke("delete_items", { ids, trashFiles });
}

/** Empty the library. Watched folders are kept, so a re-index refills it. */
export function clearLibrary(): Promise<number> {
  return invoke("clear_library");
}
