// Miller columns: each column is the previous column's selected row, opened.
//
// One implementation, two hosts — the Viewer scoped to a collector, and the
// Library reading its Hierarchy. Two copies of a cascade is precisely the
// shape this rebuild exists to remove, and the two would drift the first time
// either grew a keyboard rule.
//
// `rootId` is the collector whose contents the first column shows; null is
// the library root. A folder nested inside another is not in the library's
// root column, so a pane scoped to one has to say where to start — asking for
// a path that began with it used to return the root column alone, which the
// pane then discarded, which is why the column view came up blank.
//
// The breadcrumb lives here rather than in the host's taskbar: it is the
// cascade's own position, both hosts need it, and threading it out through a
// callback would mean the two hosts could disagree about where they are.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { treeColumns } from "../../lib/api";
import { useActiveItem } from "../../lib/activeItem";
import { useArchivaChanged } from "../../lib/events";
import * as Sel from "../../lib/selection";
import type { Row, TreeColumn } from "../../lib/types";
import { Thumbnail } from "../library/Thumbnail";

type Props = {
  /** The collector the first column shows the inside of; null is the root. */
  rootId: string | null;
  /** Said when a row is activated that nothing can open yet. */
  onAnnounce?: (row: Row) => void;
  /**
   * Start the root column inside the watched folders rather than at them.
   * The Viewer is a workspace and its flat modes already leave the folder
   * scaffolding out; the Library's Hierarchy wants it, because showing where
   * a thing actually lives is what that shape is for.
   */
  workspace?: boolean;
};

export function MillerColumns({ rootId, onAnnounce, workspace = false }: Props) {
  const [columns, setColumns] = useState<TreeColumn[]>([]);
  const [path, setPath] = useState<string[]>([]);
  const [activeCol, setActiveCol] = useState(0);
  const [lastCursor, setLastCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const rowRefs = useRef<Map<string, HTMLElement>>(new Map());
  const typeAheadRef = useRef<{ buffer: string; at: number }>({ buffer: "", at: 0 });
  const { setActive } = useActiveItem();

  // A different root is a different cascade; keeping the old path would ask
  // the backend to descend through ids that are not in it.
  useEffect(() => {
    setPath([]);
    setActiveCol(0);
  }, [rootId]);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const cols = await treeColumns(rootId, path, workspace);
      setColumns(cols);
      setError(null);
      // The backend stops the cascade at a stale or non-expandable id, so
      // what came back is the authority on how deep we actually are.
      const real = cols.slice(1).map((c) => c.scope_id as string);
      if (real.length !== path.length) setPath(real);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [rootId, path, workspace]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useArchivaChanged(refresh);

  const lastColIndex = columns.length - 1;

  useEffect(() => {
    if (lastColIndex < 0) return;
    const rows = columns[lastColIndex]?.rows ?? [];
    if (!lastCursor || !rows.some((r) => r.id === lastCursor)) {
      setLastCursor(rows[0]?.id ?? null);
    }
    // Keyed on the columns themselves; including the cursor would re-run this
    // on every move within one column.
  }, [columns]);

  useEffect(() => {
    setActiveCol((c) => Math.min(c, Math.max(lastColIndex, 0)));
  }, [lastColIndex]);

  const order = useMemo(
    () => (columns[activeCol]?.rows ?? []).map((r) => r.id),
    [columns, activeCol],
  );
  const names = useMemo(() => {
    const m = new Map<string, string>();
    (columns[activeCol]?.rows ?? []).forEach((r) => m.set(r.id, r.display_name));
    return m;
  }, [columns, activeCol]);

  function idOfColumn(colIndex: number): string | null {
    if (colIndex < path.length) return path[colIndex];
    if (colIndex === lastColIndex) return lastCursor;
    return null;
  }

  const publish = useCallback((id: string | null) => setActive(id, order), [setActive, order]);

  function selectInColumn(colIndex: number, row: Row) {
    setActiveCol(colIndex);
    publish(row.id);
    if (colIndex === lastColIndex) setLastCursor(row.id);
    // Selecting and opening are the same gesture in column view: a folder
    // shows its contents in the next column the moment it's highlighted, and
    // anything that can't be expanded closes the columns to its right.
    if (row.capabilities.includes("expand")) {
      setPath((p) => (p[colIndex] === row.id ? p : [...p.slice(0, colIndex), row.id]));
    } else {
      setPath((p) => (p.length > colIndex ? p.slice(0, colIndex) : p));
    }
  }

  function expand(colIndex: number, row: Row) {
    if (!row.capabilities.includes("expand")) return;
    setPath((p) => [...p.slice(0, colIndex), row.id]);
    setActiveCol(colIndex + 1);
  }

  function activate(colIndex: number, row: Row) {
    if (row.node_type === "collector") expand(colIndex, row);
    else onAnnounce?.(row);
  }

  function onKeyDown(e: React.KeyboardEvent) {
    const col = columns[activeCol];
    if (!col) return;
    const colOrder = col.rows.map((r) => r.id);
    const currentId = idOfColumn(activeCol);
    const rowIn = (id: string | null) => (id ? col.rows.find((r) => r.id === id) : undefined);

    switch (e.key) {
      case "ArrowUp":
      case "ArrowDown": {
        e.preventDefault();
        const seed = currentId ? Sel.click(currentId) : Sel.EMPTY_SELECTION;
        const next = Sel.moveCursor(seed, colOrder, e.key === "ArrowDown" ? 1 : -1, false);
        const landed = rowIn(next.cursor);
        if (landed) {
          selectInColumn(activeCol, landed);
          rowRefs.current.get(landed.id)?.scrollIntoView({ block: "nearest" });
        }
        return;
      }
      case "ArrowRight": {
        e.preventDefault();
        if (activeCol < lastColIndex) {
          setActiveCol(activeCol + 1);
          return;
        }
        const row = rowIn(currentId);
        if (row) expand(activeCol, row);
        return;
      }
      case "ArrowLeft":
        e.preventDefault();
        setActiveCol((c) => Math.max(0, c - 1));
        return;
      case "Enter": {
        e.preventDefault();
        const row = rowIn(currentId);
        if (row) activate(activeCol, row);
        return;
      }
    }

    // Type-ahead: any single printable character with no modifier.
    if (e.key.length !== 1 || e.metaKey || e.ctrlKey || e.altKey) return;
    const now = Date.now();
    const buf = typeAheadRef.current;
    buf.buffer = now - buf.at < 700 ? buf.buffer + e.key : e.key;
    buf.at = now;
    const match = Sel.typeAhead(colOrder, names, buf.buffer, currentId);
    const row = rowIn(match);
    if (row) {
      selectInColumn(activeCol, row);
      rowRefs.current.get(row.id)?.scrollIntoView({ block: "nearest" });
    }
  }

  const rowRef = (id: string) => (el: HTMLElement | null) => {
    if (el) rowRefs.current.set(id, el);
    else rowRefs.current.delete(id);
  };

  if (error) {
    return (
      <div className="empty">
        <span className="error">{error}</span>
      </div>
    );
  }
  if (loading && columns.length === 0) return <div className="empty">Loading…</div>;

  return (
    <div className="miller-wrap" tabIndex={0} onKeyDown={onKeyDown}>
      <div className="breadcrumb">
        {columns.map((c, i) => (
          <span key={c.scope_id ?? "root"}>
            {i > 0 && <span className="crumb-sep">›</span>}
            <button
              className={"crumb" + (i === activeCol ? " on" : "")}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setPath((p) => p.slice(0, i));
                setActiveCol(Math.max(0, i));
              }}
            >
              {c.title}
            </button>
          </span>
        ))}
      </div>

      <div className="miller">
        {columns.map((col, i) => (
          <div
            key={col.scope_id ?? "root"}
            className={"miller-col" + (i === activeCol ? " active" : "")}
            onMouseDown={() => setActiveCol(i)}
          >
            {col.rows.length === 0 ? (
              <div className="miller-empty">Empty</div>
            ) : (
              col.rows.map((row) => (
                <div
                  key={row.id}
                  ref={rowRef(row.id)}
                  className={`row${idOfColumn(i) === row.id ? " selected" : ""}`}
                  onClick={() => selectInColumn(i, row)}
                  onDoubleClick={() => activate(i, row)}
                >
                  <span className="icon">
                    <Thumbnail item={row} />
                  </span>
                  <span className="names">
                    <span className="row-name">{row.display_name}</span>
                  </span>
                  {row.capabilities.includes("expand") && <span className="miller-chevron">›</span>}
                </div>
              ))
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
