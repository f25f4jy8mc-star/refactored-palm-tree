import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";

import { listRows, pickFolder, scanFolder } from "../../lib/api";
import { openTarget } from "../../lib/capabilities";
import type { GroupBy, ListRow, SortBy } from "../../lib/types";
import { IconGlyph } from "./IconGlyph";

const GROUP_OPTIONS: { value: GroupBy; label: string }[] = [
  { value: "type", label: "Type" },
  { value: "health", label: "Tagging health" },
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

function formatSize(bytes: number | null): string {
  if (!bytes) return "";
  const mb = bytes / 1_048_576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

export type LibraryViewHandle = {
  addFolder: () => Promise<void>;
};

export const LibraryView = forwardRef<LibraryViewHandle>((_props, ref) => {
  const [rows, setRows] = useState<ListRow[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const [groupBy, setGroupBy] = useState<GroupBy>("type");
  const [sort, setSort] = useState<SortBy>("name");
  const [descending, setDescending] = useState(false);
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<string[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const listRef = useRef<HTMLDivElement>(null);

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
        query: query.trim() ? query.trim() : null,
      });
      setRows(page.rows);
      setTotal(page.total);
      setSelectedId((prev) =>
        prev && page.rows.some((r) => r.id === prev) ? prev : (page.rows[0]?.id ?? null),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [groupBy, sort, descending, expanded, query]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const selectedIndex = useMemo(
    () => rows.findIndex((r) => r.id === selectedId),
    [rows, selectedId],
  );

  function moveSelection(delta: number) {
    if (rows.length === 0) return;
    const next = Math.min(Math.max(selectedIndex + delta, 0), rows.length - 1);
    setSelectedId(rows[next].id);
  }

  function toggleExpanded(id: string) {
    setExpanded((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  }

  function openRow(row: ListRow) {
    if (row.node_type === "collector") {
      toggleExpanded(row.id);
      return;
    }
    const target = openTarget(row);
    setStatus(
      target
        ? `Would open “${row.display_name}” via ${target} — the Viewer pane isn't built yet.`
        : `“${row.display_name}” has nothing to open it with.`,
    );
  }

  function onKeyDown(e: React.KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        moveSelection(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        moveSelection(-1);
        break;
      case "Home":
        e.preventDefault();
        if (rows.length) setSelectedId(rows[0].id);
        break;
      case "End":
        e.preventDefault();
        if (rows.length) setSelectedId(rows[rows.length - 1].id);
        break;
      case "Enter": {
        e.preventDefault();
        const row = rows.find((r) => r.id === selectedId);
        if (row) openRow(row);
        break;
      }
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
      await refresh();
    } catch (e) {
      setStatus(null);
      setError(String(e));
    }
  }, [refresh]);

  useImperativeHandle(ref, () => ({ addFolder }), [addFolder]);

  let lastGroup: string | null = null;

  return (
    <div className="body">
      <div className="controls">
        <select value={groupBy} onChange={(e) => setGroupBy(e.target.value as GroupBy)}>
          {GROUP_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              Group: {o.label}
            </option>
          ))}
        </select>
        <select value={sort} onChange={(e) => setSort(e.target.value as SortBy)}>
          {SORT_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              Sort: {o.label}
            </option>
          ))}
        </select>
        <button className="btn" onClick={() => setDescending((d) => !d)}>
          {descending ? "↓ Descending" : "↑ Ascending"}
        </button>
        <div className="spacer" />
        <span className="status-line" role="status">
          {error ? (
            <span className="error">{error}</span>
          ) : (
            status ?? (loading ? "Loading…" : `${total} item${total === 1 ? "" : "s"}`)
          )}
        </span>
      </div>

      {!loading && total === 0 ? (
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
        <div className="library" ref={listRef} tabIndex={0} onKeyDown={onKeyDown}>
          {rows.map((row) => {
            const showHeader = row.group_key !== lastGroup && row.depth === 0;
            lastGroup = row.group_key;
            return (
              <div key={row.id}>
                {showHeader && row.group_label && (
                  <div className="group-head">{row.group_label}</div>
                )}
                <div
                  className={`row${row.id === selectedId ? " selected" : ""}`}
                  style={{ paddingLeft: 20 + row.depth * 20 }}
                  onClick={() => setSelectedId(row.id)}
                  onDoubleClick={() => openRow(row)}
                >
                  {row.node_type === "collector" && row.child_count > 0 ? (
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
                  )}
                  <span className="icon">
                    <IconGlyph kind={row.icon_kind} />
                  </span>
                  <span className="names">
                    <span className="row-name">{row.display_name}</span>
                    <span className="row-sub">{row.display_subtitle}</span>
                  </span>
                  {row.availability !== "present" && (
                    <span className="badge missing">{row.availability.replace("_", " ")}</span>
                  )}
                  {row.health_missing.length > 0 && (
                    <span className="badge" title={row.health_missing.join(", ")}>
                      {row.health_missing[0]}
                    </span>
                  )}
                  {row.size_bytes ? <span className="row-sub">{formatSize(row.size_bytes)}</span> : null}
                </div>
              </div>
            );
          })}
        </div>
      )}

      <div className="taskbar">
        <div className="taskbar-row">
          <span className="taskbar-name">Library</span>
          <span className="taskbar-divider" />
          <span className="taskbar-search">
            <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
              <circle cx="7" cy="7" r="5" fill="none" stroke="currentColor" strokeWidth="1.4" />
              <path d="M11 11l3.5 3.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
            </svg>
            <input
              type="search"
              placeholder="Search…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </span>
        </div>
      </div>
    </div>
  );
});

LibraryView.displayName = "LibraryView";
