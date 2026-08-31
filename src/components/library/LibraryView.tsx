import { createPortal } from "react-dom";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getViewPrefs, listRows, pickFolder, scanFolder, searchLibrary, setViewPrefs } from "../../lib/api";
import { openTarget } from "../../lib/capabilities";
import { useArchivaChanged } from "../../lib/events";
import * as Sel from "../../lib/selection";
import type { GroupBy, Hit, ListRow, Row, SortBy } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { IconGlyph } from "./IconGlyph";

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
};

export function LibraryView({ mode, isActive }: Props) {
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
  const [selection, setSelection] = useState<Sel.SelectionState>(Sel.EMPTY_SELECTION);

  const listRef = useRef<HTMLDivElement>(null);
  const typeAheadRef = useRef<{ buffer: string; at: number }>({ buffer: "", at: 0 });
  const trimmedQuery = query.trim();
  const searching = trimmedQuery.length > 0;
  const slot = useTaskbarSlot();

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
        expanded,
        query: null,
      });
      setRows(page.rows);
      setTotal(page.total);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [groupBy, sort, descending, expanded]);

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

  const visibleRows = useMemo(
    () => (kindFilter ? rows.filter((r) => r.icon_kind === kindFilter || r.node_type === kindFilter) : rows),
    [rows, kindFilter],
  );
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

  function toggleExpanded(id: string) {
    setExpanded((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  }

  function announceOpen(node: Row | ListRow) {
    const target = openTarget(node);
    setStatus(
      target
        ? `Would open “${node.display_name}” via ${target} — the Viewer pane isn't built yet.`
        : `“${node.display_name}” has nothing to open it with.`,
    );
  }

  function openRow(row: ListRow) {
    if (row.node_type === "collector") {
      toggleExpanded(row.id);
      return;
    }
    announceOpen(row);
  }

  function onRowClick(e: React.MouseEvent, id: string) {
    if (e.shiftKey) setSelection((s) => Sel.rangeClick(s, id, visibleIds));
    else if (e.metaKey || e.ctrlKey) setSelection((s) => Sel.toggleClick(s, id));
    else setSelection(Sel.click(id));
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      setSelection(Sel.selectAll(visibleIds));
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setSelection((s) => Sel.moveCursor(s, visibleIds, 1, e.shiftKey));
        return;
      case "ArrowUp":
        e.preventDefault();
        setSelection((s) => Sel.moveCursor(s, visibleIds, -1, e.shiftKey));
        return;
      case "Home":
        e.preventDefault();
        if (visibleIds.length) setSelection(Sel.click(visibleIds[0]));
        return;
      case "End":
        e.preventDefault();
        if (visibleIds.length) setSelection(Sel.click(visibleIds[visibleIds.length - 1]));
        return;
      case "Escape":
        setSelection(Sel.clear());
        return;
      case "Enter": {
        e.preventDefault();
        const id = selection.cursor;
        if (searching) {
          const hit = hits.find((h) => h.node.id === id);
          if (hit) announceOpen(hit.node);
        } else {
          const row = rows.find((r) => r.id === id);
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
      if (match) setSelection(Sel.click(match));
    }
  }

  const addFolder = useCallback(async () => {
    const dir = await pickFolder();
    if (!dir) return;
    setStatus(`Scanning ${dir}…`);
    try {
      const report = await scanFolder(dir);
      setStatus(
        `Scanned ${dir}: ${report.created} added, ${report.updated} updated, ` +
          `${report.touched} unchanged.`,
      );
    } catch (e) {
      setStatus(null);
      setError(String(e));
    }
  }, []);

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
      <select value={kindFilter ?? ""} onChange={(e) => setKindFilter(e.target.value || null)} disabled={searching}>
        {KIND_OPTIONS.map((o) => (
          <option key={o.label} value={o.value ?? ""}>
            {o.label}
          </option>
        ))}
      </select>
      {mode === "library" && (
        <select value={groupBy} onChange={(e) => setGroupBy(e.target.value as GroupBy)} disabled={searching}>
          {GROUP_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              Group: {o.label}
            </option>
          ))}
        </select>
      )}
      <select value={sort} onChange={(e) => setSort(e.target.value as SortBy)} disabled={searching}>
        {SORT_OPTIONS.map((o) => (
          <option key={o.value} value={o.value}>
            Sort: {o.label}
          </option>
        ))}
      </select>
      <button className="btn" onClick={() => setDescending((d) => !d)} disabled={searching}>
        {descending ? "↓" : "↑"}
      </button>
      <span className="taskbar-divider" />
      <button
        className={"btn" + (layout === "list" ? " on" : "")}
        title="List"
        onClick={() => setLayout("list")}
      >
        ☰
      </button>
      <button
        className={"btn" + (layout === "grid" ? " on" : "")}
        title="Grid"
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
          <button className="btn quiet" onClick={() => setSelection(Sel.clear())}>
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
        <div
          className={`library ${layout}`}
          ref={listRef}
          tabIndex={0}
          onKeyDown={onKeyDown}
        >
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
                    className={`row${Sel.isSelected(selection, hit.node.id) ? " selected" : ""}`}
                    onClick={(e) => onRowClick(e, hit.node.id)}
                    onDoubleClick={() => announceOpen(hit.node)}
                  >
                    <span className="indent" style={{ width: 16 }} />
                    <span className="icon">
                      <IconGlyph kind={hit.node.icon_kind} />
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
        <div
          className={`library ${layout}`}
          ref={listRef}
          tabIndex={0}
          onKeyDown={onKeyDown}
        >
          {visibleRows.map((row) => {
            const showHeader = row.group_key !== lastGroup && row.depth === 0;
            lastGroup = row.group_key;
            const isSelected = Sel.isSelected(selection, row.id);
            return (
              <div key={row.id}>
                {showHeader && row.group_label && layout === "list" && (
                  <div className="group-head">{row.group_label}</div>
                )}
                <div
                  className={`row${isSelected ? " selected" : ""}`}
                  style={layout === "list" ? { paddingLeft: 20 + row.depth * 20 } : undefined}
                  onClick={(e) => onRowClick(e, row.id)}
                  onDoubleClick={() => openRow(row)}
                >
                  {layout === "list" &&
                    (row.node_type === "collector" && row.child_count > 0 ? (
                      <button
                        className="disclosure"
                        onClick={(e) => {
                          e.stopPropagation();
                          toggleExpanded(row.id);
                        }}
                        aria-label={expanded.includes(row.id) ? "Collapse" : "Expand"}
                      >
                        {expanded.includes(row.id) ? "▾" : "▸"}
                      </button>
                    ) : (
                      <span className="indent" style={{ width: 16 }} />
                    ))}
                  <span className="icon">
                    <IconGlyph kind={row.icon_kind} />
                  </span>
                  <span className="names">
                    <span className="row-name">{row.display_name}</span>
                    {layout === "list" && <span className="row-sub">{row.display_subtitle}</span>}
                  </span>
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
              </div>
            );
          })}
        </div>
      )}

      {isActive && slot && createPortal(controls, slot)}
    </div>
  );
}
