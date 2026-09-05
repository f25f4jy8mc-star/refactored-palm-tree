import { createPortal } from "react-dom";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { addSource, getViewPrefs, listRows, pickFolder, searchLibrary, setViewPrefs } from "../../lib/api";
import { openTarget } from "../../lib/capabilities";
import { useActiveItem } from "../../lib/activeItem";
import { useArchivaChanged } from "../../lib/events";
import * as Expand from "../../lib/expansion";
import { nodeIdOf, nodeIdsOf, placementKeys } from "../../lib/placement";
import * as Sec from "../../lib/sections";
import * as Sel from "../../lib/selection";
import type { GroupBy, Hit, ListRow, Row, Shape, SortBy } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { Thumbnail } from "./Thumbnail";
import { MillerColumns } from "../viewer/MillerColumns";

const GROUP_OPTIONS: { value: GroupBy; label: string }[] = [
  { value: "type", label: "Type" },
  { value: "month", label: "Month added" },
  { value: "none", label: "No grouping" },
];

const SORT_OPTIONS: { value: SortBy; label: string }[] = [
  { value: "name", label: "Name" },
  { value: "date", label: "Date added" },
  { value: "captured", label: "Date captured" },
  { value: "size", label: "Size" },
  { value: "health", label: "Tagging health" },
];

/** Client-side only — `icon_kind` is already conformance-derived server-side
 * (`content_type::icon_kind`), so filtering by it is filtering by the same
 * classification every other view uses, not a second copy of the DAG. */
const KIND_OPTIONS: { value: string | null; label: string }[] = [
  { value: null, label: "All types" },
  { value: "image", label: "Images" },
  { value: "video", label: "Video" },
  { value: "audio", label: "Audio" },
  { value: "document", label: "Documents" },
  { value: "model", label: "3D" },
  { value: "note", label: "Notes" },
  { value: "folder", label: "Folders" },
  { value: "board", label: "Boards" },
];

const MATCH_SECTION: Record<Hit["match_kind"], string> = {
  name: "Name matches",
  body: "Content matches",
  via_tag: "Tag matches",
};

const HEALTH_LABEL = ["Not described", "Barely described", "Hard to search by name", "Well described"];

function formatSize(bytes: number | null): string {
  if (!bytes) return "";
  const mb = bytes / 1_048_576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

/** A group header that folds its section away. The count is of everything in
 * the section, folded or not, so a shut section still says how much is in it. */
function SectionHead({
  label,
  count,
  collapsed,
  onToggle,
}: {
  label: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      className={"group-head" + (collapsed ? " collapsed" : "")}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onToggle}
      aria-expanded={!collapsed}
    >
      <span className="group-caret">{collapsed ? "▸" : "▾"}</span>
      <span>{label}</span>
      <span className="group-count">{count}</span>
    </button>
  );
}

function Snippet({ text }: { text: string }) {
  const parts = text.split(/[‹›]/);
  return (
    <>
      {parts.map((part, i) =>
        i % 2 === 1 ? <mark key={i}>{part}</mark> : <span key={i}>{part}</span>,
      )}
    </>
  );
}

type Layout = "list" | "grid" | "column";

type Props = {
  /** Library and Scattered are the same view over the same projection
   * (p_rows), differing only in default grouping and which controls show —
   * not two components with two copies of the same list logic. */
  mode: "library" | "scattered";
  isActive: boolean;
  /** Opening a collector hands it to the Viewer pane rather than this one
   * growing a second way to browse a folder. */
  onOpenCollector?: (id: string, title: string) => void;
};

export function LibraryView({ mode, isActive, onOpenCollector }: Props) {
  const prefsScope = mode; // a view_prefs key, unrelated to p_rows' collector `scope`

  const [rows, setRows] = useState<ListRow[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [prefsLoaded, setPrefsLoaded] = useState(false);

  const [groupBy, setGroupBy] = useState<GroupBy>(mode === "scattered" ? "health" : "type");
  const [sort, setSort] = useState<SortBy>("name");
  const [descending, setDescending] = useState(false);
  const [layout, setLayout] = useState<Layout>(mode === "scattered" ? "grid" : "list");
  const [kindFilter, setKindFilter] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [expanded, setExpanded] = useState<string[]>([]);
  // Two ways of reading the library, and the reason both exist: Source is
  // every item once — what you have — and Hierarchy is where those items sit.
  // Trying to be both at once is what put an item in the list twice.
  const [shape, setShape] = useState<Shape>("source");
  // Which section headers are folded away. Their rows go; the header stays,
  // or there would be nothing left to click to bring them back.
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const [selection, setSelection] = useState<Sel.SelectionState>(Sel.EMPTY_SELECTION);

  const listRef = useRef<HTMLDivElement>(null);
  const rowRefs = useRef<Map<string, HTMLElement>>(new Map());
  const typeAheadRef = useRef<{ buffer: string; at: number }>({ buffer: "", at: 0 });
  const trimmedQuery = query.trim();
  const searching = trimmedQuery.length > 0;
  const slot = useTaskbarSlot();
  const { setActive, setSelection: publishSelection } = useActiveItem();

  // Per-scope view memory (§1.9, G13) — loaded once per mode, before the
  // first fetch, so the first render already reflects last time.
  useEffect(() => {
    setPrefsLoaded(false);
    getViewPrefs(prefsScope, "browse")
      .then((p) => {
        if (p.layout === "list" || p.layout === "grid" || p.layout === "column") {
          setLayout(p.layout);
        }
        if (p.sort) setSort(p.sort as SortBy);
        if (mode === "library" && p.group_by) setGroupBy(p.group_by as GroupBy);
        if (p.shape === "source" || p.shape === "hierarchy") setShape(p.shape);
      })
      .finally(() => setPrefsLoaded(true));
  }, [prefsScope, mode]);

  useEffect(() => {
    if (!prefsLoaded) return;
    setViewPrefs(prefsScope, "browse", {
      layout,
      sort,
      group_by: mode === "library" ? groupBy : null,
      density: null,
      shape,
    });
  }, [prefsLoaded, prefsScope, mode, layout, sort, groupBy, shape]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const page = await listRows({
        scope: null,
        groupBy,
        sort,
        descending,
        expanded,
        query: null,
        shape,
      });
      setRows(page.rows);
      setTotal(page.total);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [groupBy, sort, descending, expanded, shape]);

  useEffect(() => {
    if (!searching && prefsLoaded) refresh();
  }, [refresh, searching, prefsLoaded]);

  useArchivaChanged(refresh);

  useEffect(() => {
    if (!searching) {
      setHits([]);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const results = await searchLibrary(trimmedQuery);
        if (!cancelled) setHits(results);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [searching, trimmedQuery]);

  // A tree of tiles has nowhere to put depth, so Hierarchy is a tree or
  // columns — never the grid. Source is a list or the grid, and columns would
  // say nothing a flat listing doesn't.
  const effectiveLayout: Layout =
    shape === "hierarchy"
      ? layout === "column"
        ? "column"
        : "list"
      : layout === "column"
        ? "list"
        : layout;

  const filteredRows = useMemo(
    () => (kindFilter ? rows.filter((r) => r.icon_kind === kindFilter || r.node_type === kindFilter) : rows),
    [rows, kindFilter],
  );
  /** The sections present, counted before anything is folded. */
  const sections = useMemo(() => Sec.sectionsOf(filteredRows), [filteredRows]);
  const visibleRows = useMemo(
    () => Sec.visibleRows(filteredRows, collapsed),
    [filteredRows, collapsed],
  );
  // Selection, the cursor and type-ahead run on **placements**, not on node
  // ids. One item can be on screen twice — listed at the top level and again
  // inside an expanded Collector — and keying on the id made both rows
  // highlight and made the arrow keys jump to whichever came first. See
  // lib/placement.ts.
  const visibleKeys = useMemo(
    () =>
      searching
        ? hits.map((h) => h.node.id)
        : placementKeys(visibleRows.map((r) => ({ id: r.id, depth: r.depth }))),
    [searching, hits, visibleRows],
  );
  const names = useMemo(() => {
    const m = new Map<string, string>();
    if (searching) hits.forEach((h) => m.set(h.node.id, h.node.display_name));
    else visibleRows.forEach((r, i) => m.set(visibleKeys[i], r.display_name));
    return m;
  }, [searching, hits, visibleRows, visibleKeys]);

  /** Open or close a folder. Only one branch is open at a time — the spine
   * from the root down to whatever you last opened — so the list stays short
   * and every visible row's parentage is obvious. The placement key already
   * carries that spine, so opening is not a search. */
  function toggleExpanded(key: string) {
    setExpanded((prev) => Expand.toggle(prev, key));
  }

  function announceOpen(node: Row | ListRow) {
    const target = openTarget(node);
    setStatus(
      target
        ? `Would open “${node.display_name}” via ${target} — that viewer isn't built yet.`
        : `“${node.display_name}” has nothing to open it with.`,
    );
  }

  function openRow(row: ListRow, key: string) {
    // A collector opens in the Viewer, where it can be read as icons, a
    // list or columns. The disclosure triangle still expands it in place —
    // two different questions ("show me inside this, here" vs "take me
    // into this"), so two different gestures, as in Finder.
    if (row.node_type === "collector" && onOpenCollector) {
      onOpenCollector(row.id, row.display_name);
      return;
    }
    if (row.node_type === "collector") {
      toggleExpanded(key);
      return;
    }
    announceOpen(row);
  }

  /** Hand the focused **item** and this view's rendered order up, so the
   * Inspector and Space follow what's focused here (G16 — the order is
   * published live rather than copied, so it can't go stale).
   *
   * The cursor is a placement; what leaves this view is the node it refers
   * to, deduplicated. Nothing outside a list has any use for placements. */
  function publish(key: string | null) {
    setActive(key ? nodeIdOf(key) : null, nodeIdsOf(visibleKeys));
  }

  // Tagging applies to a selection, not to the focused row alone (C2). The
  // Inspector shows one item and writes to all of them, and this is the only
  // place that knows what "all of them" currently means. An item selected in
  // two places is still one item to tag.
  useEffect(() => {
    publishSelection(nodeIdsOf(visibleKeys.filter((k) => Sel.isSelected(selection, k))));
  }, [selection, visibleKeys, publishSelection]);

  function onRowClick(e: React.MouseEvent, key: string) {
    if (e.shiftKey) setSelection((s) => Sel.rangeClick(s, key, visibleKeys));
    else if (e.metaKey || e.ctrlKey) setSelection((s) => Sel.toggleClick(s, key));
    else setSelection(Sel.click(key));
    publish(key);
  }

  /** How many tiles fit per row right now, read the same way build17 did:
   * off the grid container's own resolved `grid-template-columns` rather
   * than computing it from container/tile widths ourselves, so it can never
   * drift from what's actually on screen as the window resizes. List mode
   * is a single column, so ↑/↓ there is just ±1 and ←/→ is a no-op. */
  function columns(): number {
    if (effectiveLayout !== "grid" || searching) return 1;
    // The grid is per section now, so the pane itself is an ordinary block.
    // Every section grid resolves to the same track count — same width, same
    // rule — so the first one on screen answers for all of them.
    const el = listRef.current?.querySelector(".grid-tiles") as HTMLElement | null;
    if (!el) return 1;
    return Math.max(1, getComputedStyle(el).gridTemplateColumns.split(" ").length);
  }

  function landOn(id: string) {
    rowRefs.current.get(id)?.scrollIntoView({ block: "nearest" });
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      setSelection(Sel.selectAll(visibleKeys));
      return;
    }
    // ⌘1/⌘2 switch layout, matching Finder/build17's view-mode shortcuts.
    if ((e.metaKey || e.ctrlKey) && (e.key === "1" || e.key === "2")) {
      e.preventDefault();
      if (shape === "source") setLayout(e.key === "1" ? "grid" : "list");
      return;
    }
    const cols = columns();
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp":
      case "ArrowRight":
      case "ArrowLeft": {
        // In a tree, ←/→ open and close rather than move sideways: → opens a
        // shut folder and then steps into it, ← closes an open one and
        // otherwise steps out to its parent. Finder's behaviour, and the
        // reason a list needs no horizontal movement of its own.
        //
        // Only the horizontal pair. ↑/↓ still walk the rows, and folding them
        // into this branch is exactly how they stopped working.
        const horizontal = e.key === "ArrowLeft" || e.key === "ArrowRight";
        if (cols === 1 && horizontal) {
          e.preventDefault();
          const key = selection.cursor;
          if (!key) return;
          const at = visibleKeys.indexOf(key);
          const row = at === -1 ? undefined : visibleRows[at];
          if (!row) return;
          const open = expanded.includes(row.id);
          const canOpen = row.node_type === "collector" && row.child_count > 0;

          if (e.key === "ArrowRight") {
            if (canOpen && !open) toggleExpanded(key);
            else if (canOpen && open) {
              // Already open: step to the first child, which is the row
              // immediately below and one level deeper.
              const child = visibleKeys[at + 1];
              if (child && visibleRows[at + 1]?.depth === row.depth + 1) {
                setSelection(Sel.click(child));
                landOn(child);
                publish(child);
              }
            }
            return;
          }
          if (canOpen && open) {
            toggleExpanded(key);
            return;
          }
          // Not an open folder: go out to whatever holds this row. The
          // placement key names the parent, so no search is needed.
          const parentKey = key.includes(">") ? key.slice(0, key.lastIndexOf(">")) : null;
          if (parentKey) {
            setSelection(Sel.click(parentKey));
            landOn(parentKey);
            publish(parentKey);
          }
          return;
        }
        e.preventDefault();
        const step =
          e.key === "ArrowDown" ? cols : e.key === "ArrowUp" ? -cols : e.key === "ArrowRight" ? 1 : -1;
        setSelection((s) => {
          const next = Sel.moveCursor(s, visibleKeys, step, e.shiftKey);
          if (next.cursor) {
            landOn(next.cursor);
            publish(next.cursor);
          }
          return next;
        });
        return;
      }
      case "Home":
        e.preventDefault();
        if (visibleKeys.length) {
          setSelection(Sel.click(visibleKeys[0]));
          landOn(visibleKeys[0]);
          publish(visibleKeys[0]);
        }
        return;
      case "End":
        e.preventDefault();
        if (visibleKeys.length) {
          const last = visibleKeys[visibleKeys.length - 1];
          setSelection(Sel.click(last));
          landOn(last);
          publish(last);
        }
        return;
      case "Escape":
        setSelection(Sel.clear());
        return;
      case "Enter": {
        e.preventDefault();
        const id = selection.cursor ? nodeIdOf(selection.cursor) : null;
        if (searching) {
          const hit = hits.find((h) => h.node.id === id);
          if (hit) announceOpen(hit.node);
        } else {
          // The row under the cursor, not merely the first row with this id:
          // the same item can be listed twice and only one of them is where
          // the cursor actually is.
          const at = visibleKeys.indexOf(selection.cursor ?? "");
          const row = at === -1 ? rows.find((r) => r.id === id) : visibleRows[at];
          if (row && selection.cursor) openRow(row, selection.cursor);
        }
        return;
      }
    }
    // Type-ahead: any single printable character with no modifier.
    if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      const now = Date.now();
      const buf = typeAheadRef.current;
      buf.buffer = now - buf.at < 700 ? buf.buffer + e.key : e.key;
      buf.at = now;
      const match = Sel.typeAhead(visibleKeys, names, buf.buffer, selection.cursor);
      if (match) {
        setSelection(Sel.click(match));
        landOn(match);
        publish(match);
      }
    }
  }

  const addFolder = useCallback(async () => {
    const dir = await pickFolder();
    if (!dir) return;
    setStatus(`Scanning ${dir}…`);
    try {
      const report = await addSource(dir);
      setStatus(
        `Scanned ${dir}: ${report.created} added, ${report.updated} updated, ` +
          `${report.touched} unchanged.`,
      );
    } catch (e) {
      setStatus(null);
      setError(String(e));
    }
  }, []);

  /** Every drawn row paired with the placement key it is selected by, so a
   * section can be assembled without losing which of two identical items a
   * tile stands for. */
  const drawn = useMemo(
    () => visibleRows.map((row, i) => ({ row, key: visibleKeys[i] })),
    [visibleRows, visibleKeys],
  );

  /** One row, as a list row or as a tile — the same element either way, so
   * the two layouts cannot drift on selection, thumbnails or double-click. */
  function itemTile(row: ListRow, key: string) {
    const isSelected = Sel.isSelected(selection, key);
    return (
      <div
        key={key}
        ref={(el) => {
          if (el) rowRefs.current.set(key, el);
          else rowRefs.current.delete(key);
        }}
        className={`row${isSelected ? " selected" : ""}`}
        style={effectiveLayout === "list" ? { paddingLeft: 20 + row.depth * 20 } : undefined}
        onClick={(e) => onRowClick(e, key)}
        onDoubleClick={() => openRow(row, key)}
      >
        {effectiveLayout === "list" &&
          (row.node_type === "collector" && row.child_count > 0 ? (
            <button
              className="disclosure"
              onClick={(e) => {
                e.stopPropagation();
                toggleExpanded(key);
              }}
              aria-label={Expand.isOpen(expanded, row.id) ? "Collapse" : "Expand"}
            >
              {Expand.isOpen(expanded, row.id) ? "▾" : "▸"}
            </button>
          ) : (
            <span className="indent" style={{ width: 16 }} />
          ))}
        <span className="icon">
          <Thumbnail item={row} />
        </span>
        <span className="names">
          <span className="row-name">{row.display_name}</span>
          {effectiveLayout === "list" && <span className="row-sub">{row.display_subtitle}</span>}
        </span>
        {effectiveLayout === "list" && row.availability !== "present" && (
          <span className="badge missing">{row.availability.replace("_", " ")}</span>
        )}
        {effectiveLayout === "list" && row.health_missing.length > 0 && (
          <span className="badge" title={row.health_missing.join(", ")}>
            {row.health_missing[0]}
          </span>
        )}
        {effectiveLayout === "list" && row.size_bytes ? (
          <span className="row-sub">{formatSize(row.size_bytes)}</span>
        ) : null}
      </div>
    );
  }

  let lastGroup: string | null = null;
  let lastMatchKind: Hit["match_kind"] | null = null;

  const controls = (
    <>
      <span className="taskbar-name">{mode === "library" ? "Library" : "Scattered"}</span>
      <span className="taskbar-divider" />
      <span className="taskbar-search">
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
          <circle cx="7" cy="7" r="5" fill="none" stroke="currentColor" strokeWidth="1.4" />
          <path d="M11 11l3.5 3.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
        </svg>
        <input type="search" placeholder="Search…" value={query} onChange={(e) => setQuery(e.target.value)} />
      </span>
      <span className="taskbar-divider" />
      <select
        value={kindFilter ?? ""}
        onChange={(e) => {
          setKindFilter(e.target.value || null);
          listRef.current?.focus();
        }}
        disabled={searching}
      >
        {KIND_OPTIONS.map((o) => (
          <option key={o.label} value={o.value ?? ""}>
            {o.label}
          </option>
        ))}
      </select>
      {mode === "library" && (
        <span className="seg" role="group" aria-label="How to read the library">
          {(["source", "hierarchy"] as Shape[]).map((v) => (
            <button
              key={v}
              className={"btn" + (shape === v ? " on" : "")}
              title={
                v === "source"
                  ? "Source — every item once, folders left out"
                  : "Hierarchy — the folder tree, one branch open at a time"
              }
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setShape(v);
                // Leaving a tree with a branch open would come back to a
                // half-open tree that no longer matches anything on screen.
                setExpanded([]);
                setSelection(Sel.clear());
                listRef.current?.focus();
              }}
              disabled={searching}
            >
              {v === "source" ? "Source" : "Hierarchy"}
            </button>
          ))}
        </span>
      )}
      {mode === "library" && shape === "source" && (
        <select
          value={groupBy}
          onChange={(e) => {
            setGroupBy(e.target.value as GroupBy);
            listRef.current?.focus();
          }}
          disabled={searching}
        >
          {GROUP_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              Group: {o.label}
            </option>
          ))}
        </select>
      )}
      <select
        value={sort}
        onChange={(e) => {
          setSort(e.target.value as SortBy);
          listRef.current?.focus();
        }}
        disabled={searching}
      >
        {SORT_OPTIONS.map((o) => (
          <option key={o.value} value={o.value}>
            Sort: {o.label}
          </option>
        ))}
      </select>
      {/* mousedown is prevented on every button below so clicking a taskbar
          control never steals focus from the list — losing focus would
          silently break arrow-key navigation until the user clicked back
          into it, which is worse than any of these looking briefly inert. */}
      <button
        className="btn"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setDescending((d) => !d)}
        disabled={searching}
      >
        {descending ? "↓" : "↑"}
      </button>
      <span className="taskbar-divider" />
      <button
        className={"btn" + (effectiveLayout === "list" ? " on" : "")}
        title={shape === "hierarchy" ? "Tree" : "List"}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setLayout("list")}
      >
        ☰
      </button>
      {/* A tree of tiles has nowhere to put depth, so the grid is not offered
          in Hierarchy at all rather than offered and inert; columns are not
          offered in Source, where they would say nothing a flat list doesn't. */}
      {shape === "hierarchy" ? (
        <button
          className={"btn" + (effectiveLayout === "column" ? " on" : "")}
          title="Columns"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setLayout("column")}
        >
          ◫
        </button>
      ) : (
        <button
          className={"btn" + (effectiveLayout === "grid" ? " on" : "")}
          title="Grid"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setLayout("grid")}
        >
          ▦
        </button>
      )}
      {mode === "library" && (
        <>
          <span className="taskbar-divider" />
          <button className="btn primary" onClick={addFolder}>
            Add Folder…
          </button>
        </>
      )}
      {mode === "scattered" && (
        <span className="health-key">
          {HEALTH_LABEL.map((label, i) => (
            <span key={label} className="health-item">
              <i className={`health-dot health-${i}`} />
              {label}
            </span>
          ))}
        </span>
      )}
      <span className="taskbar-spacer" />
      {selection.ids.size > 0 && (
        <>
          <span className="sel-count">{selection.ids.size} selected</span>
          <button
            className="btn quiet"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setSelection(Sel.clear())}
          >
            Clear
          </button>
        </>
      )}
    </>
  );

  return (
    <div className="body">
      <div className="status-line" role="status">
        {error ? (
          <span className="error">{error}</span>
        ) : searching ? (
          `${hits.length} match${hits.length === 1 ? "" : "es"} for “${trimmedQuery}”`
        ) : (
          status ?? (loading ? "Loading…" : `${visibleRows.length} of ${total} item${total === 1 ? "" : "s"}`)
        )}
      </div>

      {searching ? (
        // Search results are always a list — a hit's snippet has nowhere
        // sensible to go in a 100px tile, regardless of the browsing layout.
        <div className="library list" ref={listRef} tabIndex={0} onKeyDown={onKeyDown}>
          {hits.length === 0 ? (
            <div className="empty">
              <div>No matches for “{trimmedQuery}”.</div>
            </div>
          ) : (
            hits.map((hit) => {
              const showHeader = hit.match_kind !== lastMatchKind;
              lastMatchKind = hit.match_kind;
              return (
                <div key={hit.node.id}>
                  {showHeader && <div className="group-head">{MATCH_SECTION[hit.match_kind]}</div>}
                  <div
                    ref={(el) => {
                      if (el) rowRefs.current.set(hit.node.id, el);
                      else rowRefs.current.delete(hit.node.id);
                    }}
                    className={`row${Sel.isSelected(selection, hit.node.id) ? " selected" : ""}`}
                    onClick={(e) => onRowClick(e, hit.node.id)}
                    onDoubleClick={() => announceOpen(hit.node)}
                  >
                    <span className="indent" style={{ width: 16 }} />
                    <span className="icon">
                      <Thumbnail item={hit.node} />
                    </span>
                    <span className="names">
                      <span className="row-name">{hit.node.display_name}</span>
                      <span className="row-sub">
                        <Snippet text={hit.snippet} />
                      </span>
                    </span>
                    {hit.node.availability !== "present" && (
                      <span className="badge missing">{hit.node.availability.replace("_", " ")}</span>
                    )}
                  </div>
                </div>
              );
            })
          )}
        </div>
      ) : !loading && total === 0 ? (
        <div className="empty">
          <div>Your library is empty.</div>
          <div className="hint">
            Add a folder to index its photos, notes and documents. Nothing is copied — Archiva
            only reads what's there.
          </div>
          <button className="btn primary" onClick={addFolder}>
            Add Folder…
          </button>
        </div>
      ) : effectiveLayout === "column" ? (
        <MillerColumns rootId={null} onAnnounce={announceOpen} />
      ) : effectiveLayout === "grid" ? (
        // Finder's arrangement: a header, that group's tiles beneath it, then
        // the next header. The section owns the grid — a single grid over the
        // whole pane would make each header a grid item, which is why the
        // headers were dropped here entirely and grouping looked inert.
        <div className="library grid" ref={listRef} tabIndex={0} onKeyDown={onKeyDown}>
          {sections.map((sec) => {
            const shut = Sec.isCollapsed(collapsed, sec.key);
            return (
              <div className="grid-section" key={sec.key}>
                {/* "No grouping" returns one section with no label (`all`),
                    and a header for it would say nothing. */}
                {sec.label && (
                  <SectionHead
                    label={sec.label}
                    count={sec.count}
                    collapsed={shut}
                    onToggle={() => setCollapsed((c) => Sec.toggleSection(c, sec.key))}
                  />
                )}
                {!shut && (
                  <div className="grid-tiles">
                    {drawn
                      .filter((d) => d.row.group_key === sec.key)
                      .map(({ row, key }) => itemTile(row, key))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      ) : (
        <div
          className={`library ${effectiveLayout}`}
          ref={listRef}
          tabIndex={0}
          onKeyDown={onKeyDown}
        >
          {sections
            .filter((sec) => Sec.isCollapsed(collapsed, sec.key))
            .map((sec) => (
              <SectionHead
                key={sec.key}
                label={sec.label}
                count={sec.count}
                collapsed
                onToggle={() => setCollapsed((c) => Sec.toggleSection(c, sec.key))}
              />
            ))}
          {drawn.map(({ row, key }) => {
            const showHeader = row.group_key !== lastGroup && row.depth === 0;
            lastGroup = row.group_key;
            const section = showHeader
              ? sections.find((x) => x.key === row.group_key)
              : undefined;
            return (
              <div key={key}>
                {section && row.group_label && (
                  <SectionHead
                    label={row.group_label}
                    count={section.count}
                    collapsed={false}
                    onToggle={() => setCollapsed((c) => Sec.toggleSection(c, row.group_key))}
                  />
                )}
                {itemTile(row, key)}
              </div>
            );
          })}
        </div>
      )}

      {isActive && slot && createPortal(controls, slot)}
    </div>
  );
}
