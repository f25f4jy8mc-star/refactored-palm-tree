// The Viewer: one collector's contents, in whichever of three shapes you
// want to read it — icon (⌘1), list (⌘2), column/Miller (⌘3). Columns are
// not a separate destination; they are one way of looking at a collector,
// which is why there is one pane here and not three.
//
// Icon and list read `p_rows` scoped to the collector. Column reads
// `p_tree`, which is the same listing per column plus the cascade. Layout
// is remembered per collector in `view_prefs`, so each folder keeps how you
// last looked at it — Finder's ⌘1/2/3 memory, and the reason `view_prefs`
// is keyed by scope rather than by pane.

import { createPortal } from "react-dom";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getViewPrefs, listRows, setViewPrefs, treeColumns } from "../../lib/api";
import { openTarget } from "../../lib/capabilities";
import { useActiveItem } from "../../lib/activeItem";
import { useArchivaChanged } from "../../lib/events";
import * as Sel from "../../lib/selection";
import { isTyping, resolve } from "../../lib/shortcuts";
import type { ListRow, Row, TreeColumn } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { Thumbnail } from "../library/Thumbnail";

type Layout = "grid" | "list" | "column";

type Props = {
  /** Which collector to show; undefined is the library root. */
  scopeId?: string;
  isActive: boolean;
};

export function ViewerPane({ scopeId, isActive }: Props) {
  const [layout, setLayout] = useState<Layout>("column");
  const [prefsLoaded, setPrefsLoaded] = useState(false);

  // Flat modes (icon/list) read p_rows; column mode reads p_tree.
  const [rows, setRows] = useState<ListRow[]>([]);
  const [columns, setColumns] = useState<TreeColumn[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  // Column-mode position. `path` is the only part the backend sees.
  const [path, setPath] = useState<string[]>([]);
  const [activeCol, setActiveCol] = useState(0);
  const [lastCursor, setLastCursor] = useState<string | null>(null);
  // Flat-mode selection.
  const [selection, setSelection] = useState<Sel.SelectionState>(Sel.EMPTY_SELECTION);

  const paneRef = useRef<HTMLDivElement>(null);
  const rowRefs = useRef<Map<string, HTMLElement>>(new Map());
  const typeAheadRef = useRef<{ buffer: string; at: number }>({ buffer: "", at: 0 });
  const slot = useTaskbarSlot();
  const { setActive } = useActiveItem();

  const prefsScope = scopeId ?? "viewer:root";

  useEffect(() => {
    setPrefsLoaded(false);
    setPath([]);
    getViewPrefs(prefsScope, "viewer")
      .then((p) => {
        if (p.layout === "grid" || p.layout === "list" || p.layout === "column") {
          setLayout(p.layout);
        }
      })
      .finally(() => setPrefsLoaded(true));
  }, [prefsScope]);

  useEffect(() => {
    if (!prefsLoaded) return;
    setViewPrefs(prefsScope, "viewer", {
      layout,
      sort: null,
      group_by: null,
      density: null,
    });
  }, [prefsLoaded, prefsScope, layout]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      if (layout === "column") {
        const cols = await treeColumns(scopeId ? [scopeId, ...path] : path);
        // p_tree always leads with the library root. A pane scoped to a
        // collector *is* that collector, so its own column comes first and
        // the root is dropped — which also makes column i line up with
        // path[i] in both modes, rather than being off by one when scoped.
        const shown = scopeId ? cols.slice(1) : cols;
        setColumns(shown);
        // The backend stops the cascade at a stale or non-expandable id, so
        // what came back is the authority on how deep we actually are.
        const realPath = shown.slice(1).map((c) => c.scope_id as string);
        if (realPath.length !== path.length) setPath(realPath);
      } else {
        const page = await listRows({
          scope: scopeId ?? null,
          groupBy: "none",
          sort: "name",
          descending: false,
          expanded: [],
          query: null,
        });
        setRows(page.rows);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [layout, scopeId, path]);

  useEffect(() => {
    if (prefsLoaded) refresh();
  }, [refresh, prefsLoaded]);

  useArchivaChanged(refresh);

  const lastColIndex = columns.length - 1;

  useEffect(() => {
    if (layout !== "column" || lastColIndex < 0) return;
    const colRows = columns[lastColIndex]?.rows ?? [];
    if (!lastCursor || !colRows.some((r) => r.id === lastCursor)) {
      setLastCursor(colRows[0]?.id ?? null);
    }
    // Keyed on the columns themselves; including the cursor would re-run
    // this on every move within one column.
  }, [columns, layout]);

  useEffect(() => {
    setActiveCol((c) => Math.min(c, Math.max(lastColIndex, 0)));
  }, [lastColIndex]);

  /** The ids currently on screen, in rendered order — what arrows walk and
   * what Preview steps through. */
  const order = useMemo(
    () =>
      layout === "column"
        ? (columns[activeCol]?.rows ?? []).map((r) => r.id)
        : rows.map((r) => r.id),
    [layout, columns, activeCol, rows],
  );

  const names = useMemo(() => {
    const m = new Map<string, string>();
    const source = layout === "column" ? (columns[activeCol]?.rows ?? []) : rows;
    source.forEach((r) => m.set(r.id, r.display_name));
    return m;
  }, [layout, columns, activeCol, rows]);

  function idOfColumn(colIndex: number): string | null {
    if (colIndex < path.length) return path[colIndex];
    if (colIndex === lastColIndex) return lastCursor;
    return null;
  }

  /** Publish whatever is focused so the Inspector and Space follow it. */
  const publish = useCallback(
    (id: string | null) => setActive(id, order),
    [setActive, order],
  );

  function announceOpen(node: Row | ListRow) {
    const target = openTarget(node);
    setStatus(
      target
        ? `Would open “${node.display_name}” via ${target} — that pane isn't built yet.`
        : `“${node.display_name}” has nothing to open it with.`,
    );
  }

  /* ------------------------------------------------------ column mode */

  function selectInColumn(colIndex: number, row: Row) {
    setActiveCol(colIndex);
    publish(row.id);
    if (colIndex === lastColIndex) setLastCursor(row.id);
    // Selecting and opening are the same gesture in column view: a folder
    // shows its contents in the next column the moment it's highlighted,
    // and anything that can't be expanded closes the columns to its right.
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
    else announceOpen(row);
  }

  /* -------------------------------------------------------- flat modes */

  function onRowClick(e: React.MouseEvent, id: string) {
    if (e.shiftKey) setSelection((s) => Sel.rangeClick(s, id, order));
    else if (e.metaKey || e.ctrlKey) setSelection((s) => Sel.toggleClick(s, id));
    else setSelection(Sel.click(id));
    publish(id);
  }

  function columnsPerRow(): number {
    if (layout !== "grid") return 1;
    const el = paneRef.current?.querySelector(".viewer-grid") as HTMLElement | null;
    if (!el) return 1;
    return Math.max(1, getComputedStyle(el).gridTemplateColumns.split(" ").length);
  }

  /* ------------------------------------------------------- keyboard */

  function onKeyDown(e: React.KeyboardEvent) {
    const native = e.nativeEvent as KeyboardEvent;
    const shortcut = resolve(native);
    if (shortcut === "layoutGrid" || shortcut === "layoutList" || shortcut === "layoutColumn") {
      e.preventDefault();
      setLayout(shortcut === "layoutGrid" ? "grid" : shortcut === "layoutList" ? "list" : "column");
      return;
    }
    if (shortcut === "selectAll" && layout !== "column") {
      e.preventDefault();
      setSelection(Sel.selectAll(order));
      return;
    }
    if (shortcut === "clearSelection") {
      setSelection(Sel.clear());
      return;
    }

    if (layout === "column") return onColumnKey(e);
    return onFlatKey(e);
  }

  function onColumnKey(e: React.KeyboardEvent) {
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
    typeAhead(e, colOrder, currentId, (id) => {
      const row = rowIn(id);
      if (row) selectInColumn(activeCol, row);
    });
  }

  function onFlatKey(e: React.KeyboardEvent) {
    const cols = columnsPerRow();
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp":
      case "ArrowLeft":
      case "ArrowRight": {
        if (cols === 1 && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
          e.preventDefault();
          return;
        }
        e.preventDefault();
        const step =
          e.key === "ArrowDown" ? cols : e.key === "ArrowUp" ? -cols : e.key === "ArrowRight" ? 1 : -1;
        setSelection((s) => {
          const next = Sel.moveCursor(s, order, step, e.shiftKey);
          if (next.cursor) {
            publish(next.cursor);
            rowRefs.current.get(next.cursor)?.scrollIntoView({ block: "nearest" });
          }
          return next;
        });
        return;
      }
      case "Enter": {
        e.preventDefault();
        const row = rows.find((r) => r.id === selection.cursor);
        if (row) {
          if (row.node_type === "collector") {
            setPath([]);
            setLayout("column");
          } else announceOpen(row);
        }
        return;
      }
    }
    typeAhead(e, order, selection.cursor, (id) => {
      setSelection(Sel.click(id));
      publish(id);
    });
  }

  function typeAhead(
    e: React.KeyboardEvent,
    itemOrder: string[],
    cursor: string | null,
    land: (id: string) => void,
  ) {
    if (e.key.length !== 1 || e.metaKey || e.ctrlKey || e.altKey) return;
    const now = Date.now();
    const buf = typeAheadRef.current;
    buf.buffer = now - buf.at < 700 ? buf.buffer + e.key : e.key;
    buf.at = now;
    const match = Sel.typeAhead(itemOrder, names, buf.buffer, cursor);
    if (match) {
      land(match);
      rowRefs.current.get(match)?.scrollIntoView({ block: "nearest" });
    }
  }

  // ⌘1/2/3 must work when focus is anywhere in this pane, including the
  // taskbar controls it portals out — so it also listens at the window
  // while active, using the same shortcut table.
  useEffect(() => {
    if (!isActive) return;
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e)) return;
      const s = resolve(e);
      if (s === "layoutGrid" || s === "layoutList" || s === "layoutColumn") {
        e.preventDefault();
        setLayout(s === "layoutGrid" ? "grid" : s === "layoutList" ? "list" : "column");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [isActive]);

  const rowRef = (id: string) => (el: HTMLElement | null) => {
    if (el) rowRefs.current.set(id, el);
    else rowRefs.current.delete(id);
  };

  // Column i is reached by path.slice(0, i), so a crumb's index is exactly
  // how much of the path to keep when it's clicked.
  const crumbs = columns.map((c, i) => ({ label: c.title, index: i }));

  const layoutButton = (which: Layout, glyph: string, title: string) => (
    <button
      className={"btn" + (layout === which ? " on" : "")}
      title={`${title} (⌘${which === "grid" ? "1" : which === "list" ? "2" : "3"})`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => setLayout(which)}
    >
      {glyph}
    </button>
  );

  const controls = (
    <>
      <span className="taskbar-name">Viewer</span>
      <span className="taskbar-divider" />
      {layout === "column" ? (
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
      ) : (
        <span className="taskbar-status">
          {loading ? "Loading…" : `${rows.length} item${rows.length === 1 ? "" : "s"}`}
        </span>
      )}
      <span className="taskbar-divider" />
      {layoutButton("grid", "▦", "Icon view")}
      {layoutButton("list", "☰", "List view")}
      {layoutButton("column", "◫", "Column view")}
      <span className="taskbar-spacer" />
      {selection.ids.size > 0 && layout !== "column" && (
        <span className="sel-count">{selection.ids.size} selected</span>
      )}
      <span className="taskbar-status" role="status">
        {error ? <span className="error">{error}</span> : status}
      </span>
    </>
  );

  const flatRow = (row: ListRow) => (
    <div
      key={row.id}
      ref={rowRef(row.id)}
      className={`row${Sel.isSelected(selection, row.id) ? " selected" : ""}`}
      onClick={(e) => onRowClick(e, row.id)}
      onDoubleClick={() =>
        row.node_type === "collector" ? (setPath([]), setLayout("column")) : announceOpen(row)
      }
    >
      <span className="icon">
        <Thumbnail item={row} />
      </span>
      <span className="names">
        <span className="row-name">{row.display_name}</span>
        {layout === "list" && <span className="row-sub">{row.display_subtitle}</span>}
      </span>
      {layout === "list" && row.availability !== "present" && (
        <span className="badge missing">{row.availability.replace("_", " ")}</span>
      )}
    </div>
  );

  return (
    <div className="body">
      <div className="viewer" ref={paneRef} tabIndex={0} onKeyDown={onKeyDown}>
        {loading && rows.length === 0 && columns.length === 0 ? (
          <div className="empty">Loading…</div>
        ) : layout === "column" ? (
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
                      {row.capabilities.includes("expand") && (
                        <span className="miller-chevron">›</span>
                      )}
                    </div>
                  ))
                )}
              </div>
            ))}
          </div>
        ) : rows.length === 0 ? (
          <div className="empty">
            <div>This collector is empty.</div>
          </div>
        ) : (
          <div className={layout === "grid" ? "viewer-grid" : "viewer-list"}>
            {rows.map(flatRow)}
          </div>
        )}
      </div>

      {isActive && slot && createPortal(controls, slot)}
    </div>
  );
}
