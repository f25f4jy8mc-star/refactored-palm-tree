import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";

import { listRows, pickFolder, scanFolder, searchLibrary } from "../../lib/api";
import { openTarget } from "../../lib/capabilities";
import type { GroupBy, Hit, ListRow, Row, SortBy } from "../../lib/types";
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

const MATCH_SECTION: Record<Hit["match_kind"], string> = {
  name: "Name matches",
  body: "Content matches",
  via_tag: "Tag matches",
};

function formatSize(bytes: number | null): string {
  if (!bytes) return "";
  const mb = bytes / 1_048_576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

/** Strips the `‹…›` markers the backend wraps the matched term in and
 * renders them as emphasis instead, so the match is visible without the
 * raw delimiter characters leaking into the UI. */
function Snippet({ text }: { text: string }) {
  const parts = text.split(/[‹›]/);
  // Odd indices are always the marked span — snippet() alternates plain,
  // marked, plain, marked, ...
  return (
    <>
      {parts.map((part, i) =>
        i % 2 === 1 ? <mark key={i}>{part}</mark> : <span key={i}>{part}</span>,
      )}
    </>
  );
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
  const [hits, setHits] = useState<Hit[]>([]);
  const [expanded, setExpanded] = useState<string[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const listRef = useRef<HTMLDivElement>(null);
  const trimmedQuery = query.trim();
  const searching = trimmedQuery.length > 0;

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
      setSelectedId((prev) =>
        prev && page.rows.some((r) => r.id === prev) ? prev : (page.rows[0]?.id ?? null),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [groupBy, sort, descending, expanded]);

  useEffect(() => {
    if (!searching) refresh();
  }, [refresh, searching]);

  // p_search is a real query, not a client-side filter, so it's debounced
  // rather than fired on every keystroke.
  useEffect(() => {
    if (!searching) {
      setHits([]);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const results = await searchLibrary(trimmedQuery);
        if (!cancelled) {
          setHits(results);
          setSelectedId((prev) =>
            prev && results.some((h) => h.node.id === prev) ? prev : (results[0]?.node.id ?? null),
          );
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [searching, trimmedQuery]);

  const visibleIds = useMemo(
    () => (searching ? hits.map((h) => h.node.id) : rows.map((r) => r.id)),
    [searching, hits, rows],
  );
  const selectedIndex = useMemo(
    () => visibleIds.indexOf(selectedId ?? ""),
    [visibleIds, selectedId],
  );

  function moveSelection(delta: number) {
    if (visibleIds.length === 0) return;
    const next = Math.min(Math.max(selectedIndex + delta, 0), visibleIds.length - 1);
    setSelectedId(visibleIds[next]);
  }

  function toggleExpanded(id: string) {
    setExpanded((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
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
        if (visibleIds.length) setSelectedId(visibleIds[0]);
        break;
      case "End":
        e.preventDefault();
        if (visibleIds.length) setSelectedId(visibleIds[visibleIds.length - 1]);
        break;
      case "Enter": {
        e.preventDefault();
        if (searching) {
          const hit = hits.find((h) => h.node.id === selectedId);
          if (hit) announceOpen(hit.node);
        } else {
          const row = rows.find((r) => r.id === selectedId);
          if (row) openRow(row);
        }
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
  let lastMatchKind: Hit["match_kind"] | null = null;

  return (
    <div className="body">
      <div className="controls">
        <select value={groupBy} onChange={(e) => setGroupBy(e.target.value as GroupBy)} disabled={searching}>
          {GROUP_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              Group: {o.label}
            </option>
          ))}
        </select>
        <select value={sort} onChange={(e) => setSort(e.target.value as SortBy)} disabled={searching}>
          {SORT_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              Sort: {o.label}
            </option>
          ))}
        </select>
        <button className="btn" onClick={() => setDescending((d) => !d)} disabled={searching}>
          {descending ? "↓ Descending" : "↑ Ascending"}
        </button>
        <div className="spacer" />
        <span className="status-line" role="status">
          {error ? (
            <span className="error">{error}</span>
          ) : searching ? (
            `${hits.length} match${hits.length === 1 ? "" : "es"} for “${trimmedQuery}”`
          ) : (
            status ?? (loading ? "Loading…" : `${total} item${total === 1 ? "" : "s"}`)
          )}
        </span>
      </div>

      {searching ? (
        <div className="library" ref={listRef} tabIndex={0} onKeyDown={onKeyDown}>
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
                    className={`row${hit.node.id === selectedId ? " selected" : ""}`}
                    onClick={() => setSelectedId(hit.node.id)}
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
