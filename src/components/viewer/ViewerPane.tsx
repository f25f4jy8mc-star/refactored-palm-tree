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

import { getViewPrefs, listRows, setViewPrefs } from "../../lib/api";
import { openTarget } from "../../lib/capabilities";
import { useActiveItem } from "../../lib/activeItem";
import { useArchivaChanged } from "../../lib/events";
import * as Sel from "../../lib/selection";
import { isTyping, resolve } from "../../lib/shortcuts";
import type { ListRow, Row } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { Thumbnail } from "../library/Thumbnail";
import { MillerColumns } from "./MillerColumns";

type Layout = "grid" | "list" | "column";

type Props = {
  /** Which collector to show; undefined is the library root. */
  scopeId?: string;
  isActive: boolean;
};

export function ViewerPane({ scopeId, isActive }: Props) {
  const [layout, setLayout] = useState<Layout>("column");
  const [prefsLoaded, setPrefsLoaded] = useState(false);

  // Flat modes (icon/list) read p_rows; column mode is MillerColumns, which
  // reads p_tree and owns its own position.
  const [rows, setRows] = useState<ListRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  // Flat-mode selection.
  const [selection, setSelection] = useState<Sel.SelectionState>(Sel.EMPTY_SELECTION);

  const paneRef = useRef<HTMLDivElement>(null);
  const rowRefs = useRef<Map<string, HTMLElement>>(new Map());
  const typeAheadRef = useRef<{ buffer: string; at: number }>({ buffer: "", at: 0 });
  const slot = useTaskbarSlot();
  const { setActive, setSelection: publishSelection } = useActiveItem();

  const prefsScope = scopeId ?? "viewer:root";

  useEffect(() => {
    setPrefsLoaded(false);
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
      shape: null,
    });
  }, [prefsLoaded, prefsScope, layout]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      if (layout !== "column") {
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
  }, [layout, scopeId]);

  useEffect(() => {
    if (prefsLoaded) refresh();
  }, [refresh, prefsLoaded]);

  useArchivaChanged(refresh);

  /** The ids currently on screen, in rendered order — what arrows walk and
   * what Preview steps through. */
  const order = useMemo(() => rows.map((r) => r.id), [rows]);

  const names = useMemo(() => {
    const m = new Map<string, string>();
    rows.forEach((r) => m.set(r.id, r.display_name));
    return m;
  }, [rows]);

  /** Publish whatever is focused so the Inspector and Space follow it. */
  const publish = useCallback(
    (id: string | null) => setActive(id, order),
    [setActive, order],
  );

  // Tagging applies to a selection, not to the focused row alone (C2), and
  // the Inspector has no other way to know what this pane has selected.
  // Column mode selects exactly one row per column, so there is nothing to
  // publish beyond the active item, which `setActive` already covers.
  useEffect(() => {
    if (layout === "column") return;
    publishSelection(order.filter((id) => Sel.isSelected(selection, id)));
  }, [layout, selection, order, publishSelection]);

  function announceOpen(node: Row | ListRow) {
    const target = openTarget(node);
    setStatus(
      target
        ? `Would open “${node.display_name}” via ${target} — that pane isn't built yet.`
        : `“${node.display_name}” has nothing to open it with.`,
    );
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

    return onFlatKey(e);
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
          // A folder opens as columns, which is where a folder is legible.
          if (row.node_type === "collector") setLayout("column");
          else announceOpen(row);
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
      <span className="taskbar-status">
        {layout === "column"
          ? "Columns"
          : loading
            ? "Loading…"
            : `${rows.length} item${rows.length === 1 ? "" : "s"}`}
      </span>
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
        row.node_type === "collector" ? setLayout("column") : announceOpen(row)
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
        {layout === "column" ? (
          <MillerColumns rootId={scopeId ?? null} onAnnounce={announceOpen} />
        ) : loading && rows.length === 0 ? (
          <div className="empty">Loading…</div>
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
