import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import type {
  Added,
  Detail,
  DuplicatePair,
  Facet,
  GatherTarget,
  Hit,
  NewKind,
  Row,
  SelectionTags,
  ItemRecord,
  ListOptions,
  ListPage,
  NoteBody,
  Recheck,
  RemovalPreview,
  RemovalResult,
  ScanReport,
  Settings,
  Source,
  Space,
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

/** The Miller cascade. `root` is the collector the first column shows the
 * inside of; null is the library root. A folder nested inside another is not
 * in the library's root column, so a pane scoped to one has to start there.
 *
 * `workspace` asks for the Viewer's root column, which starts inside the
 * watched folders rather than at them — the Library's Hierarchy wants the
 * other one, where seeing the watched folders is the point. */
export function treeColumns(
  root: string | null,
  path: string[],
  workspace = false,
): Promise<TreeColumn[]> {
  return invoke("tree_columns", { root, path, workspace });
}

export function nodeDetail(id: string): Promise<Detail> {
  return invoke("node_detail", { id });
}

/* ------------------------------------------------ relating and making */

/** Put each of `others` in one arm of `item`'s compass (S4). The arm decides
 * the kind (S6): a tag in North is a tagging, a collector is membership. */
export function addToArm(item: string, compass: string, others: string[]): Promise<Added> {
  return invoke("add_to_arm", { item, compass, others });
}

export function unlinkEdge(edgeId: string): Promise<void> {
  return invoke("unlink_edge", { edgeId });
}

/** Whether this can be gathered into — null for anything that cannot,
 * including a folder mirrored from disk. */
export function gatherTarget(id: string): Promise<GatherTarget | null> {
  return invoke("gather_target", { id });
}

export function gather(ids: string[], collector: string): Promise<Added> {
  return invoke("gather", { ids, collector });
}

export function ungather(ids: string[], collector: string): Promise<number> {
  return invoke("ungather", { ids, collector });
}

/** Rows for ids a list published, in that order, skipping any now gone. */
export function rowsOf(ids: string[]): Promise<Row[]> {
  return invoke("rows_of", { ids });
}

export function selectionTags(ids: string[]): Promise<SelectionTags> {
  return invoke("selection_tags", { ids });
}

export function createItem(
  kind: NewKind,
  name: string,
  opts: { url?: string | null; into?: string | null } = {},
): Promise<Row> {
  return invoke("create_item", { kind, name, url: opts.url ?? null, into: opts.into ?? null });
}

/* -------------------------------------------------------------- spaces */

/** Every space this machine knows about, most recently opened first. */
export function listSpaces(): Promise<Space[]> {
  return invoke("list_spaces");
}

/** The space this window is looking at. Null on a first run, and null when
 * the last one's folder is not there — both are a screen, not an error. */
export function currentSpace(): Promise<Space | null> {
  return invoke("current_space");
}

/** Make a space in a folder the user chose, and open it. */
export function createSpace(name: string, path: string): Promise<Space> {
  return invoke("create_space", { name, path });
}

export function openSpace(id: string): Promise<Space> {
  return invoke("open_space", { id });
}

/** Open a folder that already holds a space — a backup drive, another
 * machine's copy. A folder with no space in it becomes one. */
export function openSpaceFolder(path: string): Promise<Space> {
  return invoke("open_space_folder", { path });
}

export function renameSpace(id: string, name: string): Promise<Space> {
  return invoke("rename_space", { id, name });
}

/** Move a space's folder, with everything in it. */
export function moveSpace(id: string, path: string): Promise<Space> {
  return invoke("move_space", { id, path });
}

/** Stop listing a space here. Never deletes the folder. */
export function forgetSpace(id: string): Promise<void> {
  return invoke("forget_space", { id });
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

/** Stages the change — nothing moves until `rescan()` applies it. Ticking a
 * folder back to where it already is cancels the staging rather than adding
 * a second one. */
export function setSourceEnabled(id: string, enabled: boolean): Promise<void> {
  return invoke("set_source_enabled", { id, enabled });
}

/** True when a tickbox is waiting on a Refresh. Derived here rather than
 * asked of the backend a second time — the list already says so. */
export function isStaged(sources: Source[]): boolean {
  return sources.some((s) => s.pending_enabled !== null);
}

/** What a source's tickbox should show: the staged answer if there is one. */
export function tickOf(s: Source): boolean {
  return s.pending_enabled ?? s.enabled;
}

/** Re-index every enabled source in one pass — there is deliberately no
 * scan-one-folder call, see `commands::rescan`. */
export function rescan(): Promise<ScanReport> {
  return invoke("rescan");
}

/* ---------------------------------------------------------- settings */

export function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

/** Not staged behind Refresh: nothing is indexed or forgotten by it, so it
 * takes effect at once. */
export function setShowLinkedFolders(show: boolean): Promise<void> {
  return invoke("set_show_linked_folders", { show });
}

/* --------------------------------------------------------- view prefs */

export function getViewPrefs(scopeId: string, paneKind: string): Promise<ViewPrefs> {
  return invoke("get_view_prefs", { scopeId, paneKind });
}

export function setViewPrefs(scopeId: string, paneKind: string, prefs: ViewPrefs): Promise<void> {
  return invoke("set_view_prefs", { scopeId, paneKind, prefs });
}

/** Show a file where it lives, in the machine's own file manager. Offered
 * only where the registry granted `reveal` — a local file that is present —
 * so this is never asked about a path the file manager could not find. */
export function revealInFileManager(path: string): Promise<void> {
  return revealItemInDir(path);
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

/** A note's text, for showing it. Null for anything that is not a note —
 * the view asks only when the registry granted `edit`. */
export function noteBody(id: string): Promise<NoteBody | null> {
  return invoke("note_body", { id });
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
