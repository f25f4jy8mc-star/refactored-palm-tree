// The Viewer's Miller-columns mode. Per the model doc, Miller is p_tree
// plus the "column" keyboard-nav variant: ↑/↓ move within a column, →
// descends into a folder landing on its first row, ← returns to the parent
// column keeping its own selection — none of that collapses the columns to
// the right, exactly like Finder: only actually picking a *different* row
// in a shallower column truncates what's deeper than it.
//
// `path` is the only state that matters to the backend — each entry is a
// collector id the user has drilled into. Everything else here (activeCol,
// lastCursor) is pure UI position and never leaves this component.

import { createPortal } from "react-dom";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { treeColumns } from "../../lib/api";
import { openTarget } from "../../lib/capabilities";
import * as Sel from "../../lib/selection";
import type { Row, TreeColumn } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { Thumbnail } from "../library/Thumbnail";

type Props = {
  isActive: boolean;
};

export function MillerView({ isActive }: Props) {
  const [path, setPath] = useState<string[]>([]);
  const [columns, setColumns] = useState<TreeColumn[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [activeCol, setActiveCol] = useState(0);
  const [lastCursor, setLastCursor] = useState<string | null>(null);

  const colRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const rowRefs = useRef<Map<string, HTMLElement>>(new Map());
  const typeAheadRef = useRef<{ buffer: string; at: number }>({ buffer: "", at: 0 });
  const slot = useTaskbarSlot();

  const refresh = useCallback(async (p: string[]) => {
    setLoading(true);
    setError(null);
    try {
      const cols = await treeColumns(p);
      setColumns(cols);
      // The cascade may have stopped short (a stale id, a leaf that can't
      // expand) — trust what came back rather than the path we asked for.
      const realPath = cols.slice(1).map((c) => c.scope_id as string);
      if (realPath.length !== p.length) setPath(realPath);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh(path);
  }, [path, refresh]);

  const lastColIndex = columns.length - 1;

  // Land on the first row of a freshly-opened column, and keep the cursor
  // sane if the list underneath it changed shape.
  useEffect(() => {
    if (lastColIndex < 0) return;
    const rows = columns[lastColIndex]?.rows ?? [];
    if (!lastCursor || !rows.some((r) => r.id === lastCursor)) {
      setLastCursor(rows[0]?.id ?? null);
    }
    // Deliberately keyed on `columns` alone: `lastColIndex`/`lastCursor` are
    // read, not depended on — including them would re-run this on every
    // keystroke inside the same column rather than only when it changes.
  }, [columns]);

  useEffect(() => {
    setActiveCol((c) => Math.min(c, Math.max(lastColIndex, 0)));
  }, [lastColIndex]);

  function idOf(colIndex: number): string | null {
    if (colIndex < path.length) return path[colIndex];
    if (colIndex === lastColIndex) return lastCursor;
    return null;
  }

  function rowIn(colIndex: number, id: string | null): Row | undefined {
    return id ? columns[colIndex]?.rows.find((r) => r.id === id) : undefined;
  }

  function selectInColumn(colIndex: number, id: string) {
    setActiveCol(colIndex);
    if (colIndex === lastColIndex) {
      setLastCursor(id);
      return;
    }
    if (path[colIndex] === id) return; // no actual change
    // Picking a different row in an already-expanded column truncates
    // everything to its right — those columns described a drill-down that
    // no longer applies.
    setPath((p) => [...p.slice(0, colIndex), id]);
  }

  function expand(colIndex: number, row: Row) {
    if (!row.capabilities.includes("expand")) return;
    setPath((p) => [...p.slice(0, colIndex), row.id]);
    setActiveCol(colIndex + 1);
  }

  function announceOpen(row: Row) {
    const target = openTarget(row);
    setStatus(
      target
        ? `Would open “${row.display_name}” via ${target} — that pane isn't built yet.`
        : `“${row.display_name}” has nothing to open it with.`,
    );
  }

  function activate(colIndex: number, row: Row) {
    if (row.node_type === "collector") expand(colIndex, row);
    else announceOpen(row);
  }

  const names = useMemo(() => {
    const m = new Map<string, string>();
    columns[activeCol]?.rows.forEach((r) => m.set(r.id, r.display_name));
    return m;
  }, [columns, activeCol]);

  function onKeyDown(e: React.KeyboardEvent) {
    const col = columns[activeCol];
    if (!col) return;
    const order = col.rows.map((r) => r.id);
    const currentId = idOf(activeCol);

    switch (e.key) {
      case "ArrowUp":
      case "ArrowDown": {
        e.preventDefault();
        const delta = e.key === "ArrowDown" ? 1 : -1;
        const seed = currentId ? Sel.click(currentId) : Sel.EMPTY_SELECTION;
        const next = Sel.moveCursor(seed, order, delta, false);
        if (next.cursor) {
          selectInColumn(activeCol, next.cursor);
          rowRefs.current.get(next.cursor)?.scrollIntoView({ block: "nearest" });
        }
        return;
      }
      case "ArrowRight": {
        e.preventDefault();
        if (activeCol < lastColIndex) {
          setActiveCol(activeCol + 1);
          return;
        }
        const row = rowIn(activeCol, currentId);
        if (row) expand(activeCol, row);
        return;
      }
      case "ArrowLeft":
        e.preventDefault();
        setActiveCol((c) => Math.max(0, c - 1));
        return;
      case "Enter": {
        e.preventDefault();
        const row = rowIn(activeCol, currentId);
        if (row) activate(activeCol, row);
        return;
      }
    }
    if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      const now = Date.now();
      const buf = typeAheadRef.current;
      buf.buffer = now - buf.at < 700 ? buf.buffer + e.key : e.key;
      buf.at = now;
      const match = Sel.typeAhead(order, names, buf.buffer, currentId);
      if (match) selectInColumn(activeCol, match);
    }
  }

  const crumbs: { label: string; index: number }[] = [
    { label: "Library", index: 0 },
    ...columns.slice(1).map((c, i) => ({ label: c.title, index: i + 1 })),
  ];

  const controls = (
    <>
      <span className="taskbar-name">Viewer</span>
      <span className="taskbar-divider" />
      <span className="breadcrumb">
        {crumbs.map((c, i) => (
          <span key={c.index}>
            {i > 0 && <span className="crumb-sep">›</span>}
            <button
              className="crumb"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setPath((p) => p.slice(0, c.index))}
            >
              {c.label}
            </button>
          </span>
        ))}
      </span>
      <span className="taskbar-spacer" />
      <span className="taskbar-status" role="status">
        {error ? <span className="error">{error}</span> : status}
      </span>
    </>
  );

  return (
    <div className="body">
      <div className="miller" tabIndex={0} onKeyDown={onKeyDown}>
        {loading && columns.length === 0 ? (
          <div className="empty">Loading…</div>
        ) : columns.length === 1 && columns[0].rows.length === 0 ? (
          <div className="empty">
            <div>Nothing here yet.</div>
            <div className="hint">Items that aren't inside a collector don't appear in Miller — add one from Library.</div>
          </div>
        ) : (
          columns.map((col, i) => (
            <div
              key={col.scope_id ?? "root"}
              ref={(el) => {
                if (el) colRefs.current.set(i, el);
                else colRefs.current.delete(i);
              }}
              className={"miller-col" + (i === activeCol ? " active" : "")}
              onMouseDown={() => setActiveCol(i)}
            >
              {col.rows.length === 0 ? (
                <div className="miller-empty">Empty</div>
              ) : (
                col.rows.map((row) => {
                  const selected = idOf(i) === row.id;
                  const expandable = row.capabilities.includes("expand");
                  return (
                    <div
                      key={row.id}
                      ref={(el) => {
                        if (el) rowRefs.current.set(row.id, el);
                        else rowRefs.current.delete(row.id);
                      }}
                      className={`row${selected ? " selected" : ""}`}
                      onClick={() => selectInColumn(i, row.id)}
                      onDoubleClick={() => activate(i, row)}
                    >
                      <span className="icon">
                        <Thumbnail item={row} />
                      </span>
                      <span className="names">
                        <span className="row-name">{row.display_name}</span>
                      </span>
                      {expandable && <span className="miller-chevron">›</span>}
                    </div>
                  );
                })
              )}
            </div>
          ))
        )}
      </div>

      {isActive && slot && createPortal(controls, slot)}
    </div>
  );
}
