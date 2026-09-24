import { createPortal } from "react-dom";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { addSource, getViewPrefs, listRows, pickFolder, searchLibrary, setViewPrefs } from "../../lib/api";
import { openTarget } from "../../lib/capabilities";
import { useActiveItem } from "../../lib/activeItem";
import { useArchivaChanged } from "../../lib/events";
import * as Sec from "../../lib/sections";
import * as Sel from "../../lib/selection";
import type { GroupBy, Hit, ListRow, Row, SortBy } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { Thumbnail } from "./Thumbnail";
import { dragIds, itemDrag } from "../../lib/drag";

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

type Layout = "list" | "grid";

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
  const { setActive, setSelection: publishSelection, reveal } = useActiveItem();

  // Per-scope view memory (§1.9, G13) — loaded once per mode, before the
  // first fetch, so the first render already reflects last time.
  useEffect(() => {
    setPrefsLoaded(false);
    getViewPrefs(prefsScope, "browse")
      .then((p) => {
        if (p.layout === "list" || p.layout === "grid") setLayout(p.layout);
        if (p.sort) setSort(p.sort as SortBy);
        if (mode === "library" && p.group_by) setGroupBy(p.group_by as GroupBy);
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
      shape: null,
    });
  }, [prefsLoaded, prefsScope, mode, layout, sort, groupBy]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const page = await listRows({
        scope: null,
        groupBy,
        sort,
        descending,
        expanded: [],
        query: null,
      });
      setRows(page.rows);
      setTotal(page.total);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [groupBy, sort, descending]);

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
  /** What the arrows walk, in drawn order. Node ids, plainly: the listing is
   * flat and holds one row per node, so there is nothing here for a row to be
   * confused with. (It used to be a placement key, because the tree could put
   * one item on screen twice.) */
  const visibleIds = useMemo(
    () => (searching ? hits.map((h) => h.node.id) : visibleRows.map((r) => r.id)),
    [searching, hits, visibleRows],
  );
  const names = useMemo(() => {
    const m = new Map<string, string>();
    if (searching) hits.forEach((h) => m.set(h.node.id, h.node.display_name));
    else visibleRows.forEach((r) => m.set(r.id, r.display_name));
    return m;
  }, [searching, hits, visibleRows]);

  // "Open in Library", answered. It acts on the *asking* — a serial that
  // changes each time — rather than on the active item, which this pane
  // publishes itself and would otherwise be chasing.
  //
  // Whatever is hiding the row is undone first: a reveal that leaves you
  // looking at a filter is not a reveal. That is what the gesture means in
  // Finder too, and it is why the filter and the fold are cleared here
  // rather than the row being reported as missing.
  const lastReveal = useRef(0);
  useEffect(() => {
    if (!reveal || reveal.at === lastReveal.current) return;
    lastReveal.current = reveal.at;
    const row = rows.find((r) => r.id === reveal.id);
    if (!row) return;
    setQuery("");
    setKindFilter(null);
    setCollapsed((c) => c.filter((k) => k !== row.group_key));
    setSelection(Sel.click(row.id));
    // After the state above has been drawn, or the row is not on screen to
    // scroll to yet.
    requestAnimationFrame(() => {
      rowRefs.current.get(row.id)?.scrollIntoView({ block: "center" });
      listRef.current?.focus();
    });
  }, [reveal, rows]);

  function announceOpen(node: Row | ListRow) {
    const target = openTarget(node);
    setStatus(
      target
        ? `Would open “${node.display_name}” via ${target} — that viewer isn't built yet.`
        : `“${node.display_name}” has nothing to open it with.`,
    );
  }

  /** A collector opens in the Viewer, which is where hierarchy lives: a
   * folder there reads like a filesystem, with columns cascading as deep as
   * it goes. Nothing opens inside this pane any more — the Library says what
   * you have, not where it sits. */
  function openRow(row: ListRow) {
    if (row.node_type === "collector" && onOpenCollector) {
      onOpenCollector(row.id, row.display_name);
      return;
    }
    announceOpen(row);
  }

  /** Hand the focused item and this view's rendered order up, so the
   * Inspector and Space follow what's focused here (G16 — the order is
   * published live rather than copied, so it can't go stale). */
  function publish(id: string | null) {
    setActive(id, visibleIds);
  }

  /** What is selected, in the order it is drawn — what a drag carries and
   * what the Inspector tags. */
  function selectedIds(): string[] {
    return visibleIds.filter((id) => Sel.isSelected(selection, id));
  }

  // Tagging applies to a selection, not to the focused row alone (C2). The
  // Inspector shows one item and writes to all of them, and this is the only
  // place that knows what "all of them" currently means.
  useEffect(() => {
    publishSelection(selectedIds());
  }, [selection, visibleIds, publishSelection]);

  function onRowClick(e: React.MouseEvent, id: string) {
    if (e.shiftKey) setSelection((s) => Sel.rangeClick(s, id, visibleIds));
    else if (e.metaKey || e.ctrlKey) setSelection((s) => Sel.toggleClick(s, id));
    else setSelection(Sel.click(id));
    publish(id);
  }

  /** How many tiles fit per row right now, read the same way build17 did:
   * off the grid container's own resolved `grid-template-columns` rather
   * than computing it from container/tile widths ourselves, so it can never
   * drift from what's actually on screen as the window resizes. List mode
   * is a single column, so ↑/↓ there is just ±1 and ←/→ is a no-op. */
  function columns(): number {
    if (layout !== "grid" || searching) return 1;
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
      setSelection(Sel.selectAll(visibleIds));
      return;
    }
    // ⌘1/⌘2 switch layout, matching Finder/build17's view-mode shortcuts.
    if ((e.metaKey || e.ctrlKey) && (e.key === "1" || e.key === "2")) {
      e.preventDefault();
      setLayout(e.key === "1" ? "grid" : "list");
      return;
    }
    const cols = columns();
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp":
      case "ArrowRight":
      case "ArrowLeft": {
        // A flat listing has no branches to open, so ←/→ are movement and
        // nothing else — in the grid they step a tile, and in the list, where
        // there is one column, they do nothing at all.
        if (cols === 1 && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
          e.preventDefault();
          return;
        }
        e.preventDefault();
        const step =
          e.key === "ArrowDown" ? cols : e.key === "ArrowUp" ? -cols : e.key === "ArrowRight" ? 1 : -1;
        setSelection((s) => {
          const next = Sel.moveCursor(s, visibleIds, step, e.shiftKey);
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
        if (visibleIds.length) {
          setSelection(Sel.click(visibleIds[0]));
          landOn(visibleIds[0]);
          publish(visibleIds[0]);
        }
        return;
      case "End":
        e.preventDefault();
        if (visibleIds.length) {
          const last = visibleIds[visibleIds.length - 1];
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
        const id = selection.cursor;
        if (!id) return;
        if (searching) {
          const hit = hits.find((h) => h.node.id === id);
          if (hit) announceOpen(hit.node);
        } else {
          const row = visibleRows.find((r) => r.id === id);
          if (row) openRow(row);
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
      const match = Sel.typeAhead(visibleIds, names, buf.buffer, selection.cursor);
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

  /** One row, as a list row or as a tile — the same element either way, so
   * the two layouts cannot drift on selection, thumbnails or double-click.
   *
   * There is no disclosure triangle: a folder is opened, not unfolded here.
   * Double-clicking one hands it to the Viewer. */
  function itemTile(row: ListRow) {
    const isSelected = Sel.isSelected(selection, row.id);
    return (
      <div
        key={row.id}
        ref={(el) => {
          if (el) rowRefs.current.set(row.id, el);
          else rowRefs.current.delete(row.id);
        }}
        className={`row${isSelected ? " selected" : ""}`}
        onClick={(e) => onRowClick(e, row.id)}
        onDoubleClick={() => openRow(row)}
        {...itemDrag(() => dragIds(row.id, selectedIds()))}
      >
        <span className="icon">
          <Thumbnail item={row} />
        </span>
        <span className="names">
          <span className="row-name">{row.display_name}</span>
          {layout === "list" && <span className="row-sub">{row.display_subtitle}</span>}
        </span>
        {layout === "list" && row.node_type === "collector" && (
          <span className="row-sub">
            {row.child_count} item{row.child_count === 1 ? "" : "s"}
          </span>
        )}
        {layout === "list" && row.availability !== "present" && (
          <span className="badge missing">{row.availability.replace("_", " ")}</span>
        )}
        {layout === "list" && row.health_missing.length > 0 && (
          <span className="badge" title={row.health_missing.join(", ")}>
            {row.health_missing[0]}
          </span>
        )}
        {layout === "list" && row.size_bytes ? (
          <span className="row-sub">{formatSize(row.size_bytes)}</span>
        ) : null}
      </div>
    );
  }

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
        className={"btn" + (layout === "list" ? " on" : "")}
        title="List"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setLayout("list")}
      >
        ☰
      </button>
      {/* Two layouts, and no third: columns are how you read the *inside* of
          a collector, and the Library is not inside one. */}
      <button
        className={"btn" + (layout === "grid" ? " on" : "")}
        title="Grid"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setLayout("grid")}
      >
        ▦
      </button>
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
      ) : (
        // Both layouts are the same shape: a header, that section's rows or
        // tiles beneath it, then the next header. Finder's arrangement, and
        // the reason the grid owns its grid per section — one grid over the
        // whole pane would make each header a grid item.
        <div className={`library ${layout}`} ref={listRef} tabIndex={0} onKeyDown={onKeyDown}>
          {sections.map((sec) => {
            const shut = Sec.isCollapsed(collapsed, sec.key);
            return (
              <div className="section" key={sec.key}>
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
                  <div className={layout === "grid" ? "grid-tiles" : "list-rows"}>
                    {visibleRows.filter((r) => r.group_key === sec.key).map(itemTile)}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {isActive && slot && createPortal(controls, slot)}
    </div>
  );
}
