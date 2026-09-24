import { useCallback, useEffect, useRef, useState } from "react";

import "./App.css";
import Dock, { DockHandle, PanelKind, PanelParams } from "./dock/Dock";
import { Rail, type Flyout } from "./dock/Rail";
import { TaskBar } from "./dock/TaskBar";
import { ActiveItemProvider, useActiveItem } from "./lib/activeItem";
import { addToArm, currentSpace, gather, gatherTarget, rowsOf } from "./lib/api";
import { useArchivaChanged } from "./lib/events";
import { LIST_OWNING_PANES, isTyping, resolve } from "./lib/shortcuts";
import { barActions, barStatus, type ActionId, type BarContext } from "./lib/taskbarActions";
import type { GatherTarget } from "./lib/types";
import { WorkbenchProvider, useWorkbench } from "./lib/workbench";
import { CreatePalette } from "./components/create/CreatePalette";
import { TagPopup } from "./components/tags/TagPopup";
import { LibraryView } from "./components/library/LibraryView";
import { ViewerPane } from "./components/viewer/ViewerPane";
import { InspectorView } from "./components/inspector/InspectorView";
import { PreviewOverlay } from "./components/preview/PreviewOverlay";
import { DeleteDialog } from "./components/removal/DeleteDialog";
import { SourcesFlyout } from "./components/sources/SourcesFlyout";
import { NoSpace } from "./components/spaces/SpacesPanel";
import { TagsFlyout } from "./components/tags/TagsFlyout";

function Shell() {
  const dockRef = useRef<DockHandle | null>(null);
  const [activeKind, setActiveKind] = useState<PanelKind | null>("library");
  const [previewOpen, setPreviewOpen] = useState(false);
  const [flyout, setFlyout] = useState<Flyout | null>(null);
  const [deleting, setDeleting] = useState<string[] | null>(null);
  // The two popups the taskbar and ⌘N/⌘T open. Tagging holds the ids it was
  // opened on, so a selection changing behind it does not change what it tags.
  const [tagging, setTagging] = useState<string[] | null>(null);
  const [creating, setCreating] = useState(false);
  // "Link N to this…" asks which arm, from a small menu over the bar.
  const [linkMenu, setLinkMenu] = useState<{ x: number; y: number } | null>(null);
  // A line of feedback from a bar action — what was put where, or why not.
  const [notice, setNotice] = useState<string | null>(null);
  const bench = useWorkbench();
  const [scope, setScope] = useState<GatherTarget | null>(null);
  const [inspecting, setInspecting] = useState<
    { id: string; name: string; collector: boolean } | null
  >(null);
  // Which library this window is looking at. Null is a first run, or a space
  // whose drive is not plugged in — neither is an error, and both are the
  // same screen. Everything below needs a space to read from, so nothing
  // else is drawn until there is one.
  const [space, setSpace] = useState<{ name: string } | null | undefined>(undefined);
  const active = useActiveItem();
  const activeId = active.id;
  // The provider hands out a fresh object every render on purpose (it is
  // what pushes updates through dockview's portals), so the shortcut effect
  // reads it through a ref rather than listing it as a dependency — one
  // stable listener instead of a new one on every keystroke.
  const activeRef = useRef(active);
  activeRef.current = active;
  const modalRef = useRef(false);

  const readSpace = useCallback(() => {
    currentSpace()
      .then((s) => setSpace(s))
      .catch(() => setSpace(null));
  }, []);

  useEffect(readSpace, [readSpace]);
  // Creating, opening, moving or forgetting a space all emit the same change
  // event every pane already listens to.
  useArchivaChanged(readSpace);

  // What the bar needs to phrase its offers: the collector the active Viewer
  // is inside — asked of the backend, which is the one place that decides
  // whether it can take members — and the name of what the Inspector shows.
  //
  // Answers can arrive out of order — focus moves faster than a reply — so
  // each is kept only if nothing newer was asked since.
  const contextSeq = useRef(0);
  const readContext = useCallback(() => {
    const seq = ++contextSeq.current;
    const current = () => seq === contextSeq.current;
    const scopeId = activeKind === "viewer" ? bench.paneScope : null;
    if (scopeId) {
      gatherTarget(scopeId)
        .then((t) => current() && setScope(t))
        .catch(() => current() && setScope(null));
    } else {
      setScope(null);
    }
    if (activeKind === "inspector" && activeId) {
      rowsOf([activeId])
        .then(
          (r) =>
            current() &&
            setInspecting(
              r[0]
                ? { id: r[0].id, name: r[0].display_name, collector: r[0].node_type === "collector" }
                : null,
            ),
        )
        .catch(() => current() && setInspecting(null));
    } else {
      setInspecting(null);
    }
  }, [activeKind, bench.paneScope, activeId]);
  useEffect(readContext, [readContext]);
  useArchivaChanged(readContext);

  // A notice is about the gesture that caused it, so it does not outlive
  // the next one by long.
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(t);
  }, [notice]);

  const barCtx: BarContext = {
    pane: activeKind,
    selection: active.selection,
    tray: bench.tray,
    scope,
    inspecting,
  };
  const barRef = useRef(barCtx);
  barRef.current = barCtx;

  const onAction = useCallback(
    async (id: ActionId, anchor?: DOMRect) => {
      const ctx = barRef.current;
      try {
        switch (id) {
          case "new":
            setCreating(true);
            return;
          case "tag":
            if (ctx.selection.length) setTagging(ctx.selection);
            return;
          case "toTray":
            bench.addToTray(ctx.selection);
            return;
          case "inspect":
            dockRef.current?.open("inspector", "Inspector", undefined, false);
            return;
          case "delete":
            if (ctx.selection.length) setDeleting(ctx.selection);
            return;
          case "gather": {
            if (!ctx.scope) return;
            const target = ctx.scope;
            const ids = ctx.tray.filter((x) => x !== target.id);
            const r = await gather(ids, target.id);
            bench.clearTray();
            setNotice(
              `${r.created} added to ${target.name}` +
                (r.existed ? ` · ${r.existed} already there` : "") +
                (r.refused.length ? ` · ${r.refused.length} refused` : ""),
            );
            return;
          }
          case "link":
            if (anchor) setLinkMenu({ x: anchor.left, y: anchor.top });
            return;
        }
      } catch (e) {
        setNotice(String(e));
      }
    },
    [bench],
  );

  const linkTrayTo = useCallback(
    async (dir: string) => {
      setLinkMenu(null);
      const ctx = barRef.current;
      if (!ctx.inspecting) return;
      const ids = ctx.tray.filter((x) => x !== ctx.inspecting?.id);
      try {
        const r = await addToArm(ctx.inspecting.id, dir, ids);
        bench.clearTray();
        setNotice(
          `${r.created} linked` +
            (r.existed ? ` · ${r.existed} already there` : "") +
            (r.refused.length ? ` · ${r.refused.map(([, why]) => why).join("; ")}` : ""),
        );
      } catch (e) {
        setNotice(String(e));
      }
    },
    [bench],
  );

  const renderPanel = useCallback((params: PanelParams, isActive: boolean) => {
    switch (params.kind) {
      case "library":
        return (
          <LibraryView
            mode="library"
            isActive={isActive}
            // Always a new Viewer. Reusing an open one meant a folder you
            // had left on screen was replaced by the one you just opened.
            onOpenCollector={(id, title) => dockRef.current?.open("viewer", title, id, true)}
          />
        );
      case "scattered":
        return (
          <LibraryView
            mode="scattered"
            isActive={isActive}
            // Always a new Viewer. Reusing an open one meant a folder you
            // had left on screen was replaced by the one you just opened.
            onOpenCollector={(id, title) => dockRef.current?.open("viewer", title, id, true)}
          />
        );
      case "viewer":
        return <ViewerPane scopeId={params.scopeId} isActive={isActive} />;
      case "inspector":
        return (
          <InspectorView
            isActive={isActive}
            // The Inspector decides what is applicable; opening a pane is
            // the shell's business. A Viewer opens fresh, as it does from
            // everywhere else, so a folder you left on screen is never
            // replaced by the one you just asked for.
            onOpen={(destination, node) => {
              if (destination === "viewer") {
                dockRef.current?.open("viewer", node.display_name, node.id, true);
              }
              // The Library shows the item where it already is, so an open
              // pane is brought forward rather than a second one made. This
              // is the retargeting path `Dock.open` kept for "the moment
              // something wants it" — something does now.
              if (destination === "library") {
                dockRef.current?.open("library", "Library", undefined, false);
              }
            }}
          />
        );
    }
  }, []);

  // The app-wide half of the shortcut table. View-scoped keys (⌘1/2/3,
  // ⌘A, arrows) are handled by the focused view, which resolves them
  // against the same table — see lib/shortcuts.ts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const shortcut = resolve(e);
      if (!shortcut) return;
      // A popup owns the keyboard while it is open — Space behind the tag
      // popup must not open a preview it is sitting on top of.
      if (modalRef.current) return;
      // Space must never swallow a keystroke meant for a text field.
      if ((shortcut === "preview" || shortcut === "focusSearch") && isTyping(e)) return;

      switch (shortcut) {
        case "preview":
          if (!activeId || previewOpen) return;
          e.preventDefault();
          setPreviewOpen(true);
          return;
        case "closePanel":
          e.preventDefault();
          dockRef.current?.closeActive();
          return;
        case "splitRight":
          e.preventDefault();
          dockRef.current?.split("right");
          return;
        case "splitDown":
          e.preventDefault();
          dockRef.current?.split("below");
          return;
        case "cycleGroupForward":
          e.preventDefault();
          dockRef.current?.cycleGroup(1);
          return;
        case "cycleGroupBack":
          e.preventDefault();
          dockRef.current?.cycleGroup(-1);
          return;
        case "deleteSelection": {
          // Whatever the focused list has selected — the same list the
          // Inspector tags, so what gets removed is what you can see is
          // chosen. The dialog does the counting and the asking.
          if (isTyping(e)) return;
          const ids = activeRef.current.selection;
          if (ids.length === 0 || deleting) return;
          e.preventDefault();
          setDeleting(ids);
          return;
        }
        case "stepNext":
        case "stepPrev": {
          // Only for a pane with no list of its own; a Library or Viewer
          // pane moves its own cursor and must not be moved twice.
          if (isTyping(e)) return;
          if (!activeKind || LIST_OWNING_PANES.includes(activeKind as (typeof LIST_OWNING_PANES)[number])) return;
          const next = activeRef.current.step(shortcut === "stepNext" ? 1 : -1);
          if (!next) return;
          e.preventDefault();
          activeRef.current.setActive(next);
          return;
        }
        case "create":
          e.preventDefault();
          setCreating(true);
          return;
        case "tag": {
          e.preventDefault();
          const ids = activeRef.current.selection;
          if (ids.length) setTagging(ids);
          return;
        }
        case "focusSearch": {
          e.preventDefault();
          const search = document.querySelector<HTMLInputElement>(".taskbar-search input");
          search?.focus();
          search?.select();
          return;
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeId, previewOpen, activeKind, deleting]);

  modalRef.current = !!(tagging || creating || deleting);

  // `undefined` is "not asked yet" — drawing the first-run screen for the
  // instant before the answer arrives would flash it on every launch.
  if (space === undefined) return <div className="shell" />;
  if (space === null) return <NoSpace />;

  return (
    <div className="shell" onClick={() => flyout && setFlyout(null)}>
      <Rail
        activeKind={activeKind}
        flyout={flyout}
        // The rail adds a tab of that kind; it never jumps to one already
        // open. Two Library panes side by side is a layout, not a mistake.
        onOpen={(kind, title) => dockRef.current?.open(kind, title, undefined, true)}
        onToggleFlyout={(which) => setFlyout((f) => (f === which ? null : which))}
      />
      {flyout === "sources" && <SourcesFlyout onClose={() => setFlyout(null)} />}
      {flyout === "tags" && <TagsFlyout onClose={() => setFlyout(null)} />}
      <div className="main">
        <div className="dock-area">
          <Dock
            renderPanel={renderPanel}
            onActivePanelChange={(_title, kind) => setActiveKind(kind)}
            onReady={(handle) => (dockRef.current = handle)}
          />
        </div>
        <TaskBar
          actions={barActions(barCtx)}
          status={notice ?? barStatus(barCtx)}
          onAction={onAction}
          onTrayOpen={(id) => active.revealItem(id)}
        />
      </div>
      {previewOpen && activeId && <PreviewOverlay onClose={() => setPreviewOpen(false)} />}
      {tagging && <TagPopup ids={tagging} onClose={() => setTagging(null)} />}
      {creating && (
        <CreatePalette
          scope={scope}
          onClose={() => setCreating(false)}
          onCreated={(row) => {
            // What you just made is what you are looking at next: the
            // Inspector follows it and a list that can show it goes to it.
            active.revealItem(row.id);
            setNotice(`Made ${row.display_name}`);
          }}
        />
      )}
      {linkMenu && (
        <LinkMenu
          at={linkMenu}
          intoCollector={!!inspecting?.collector}
          onPick={linkTrayTo}
          onClose={() => setLinkMenu(null)}
        />
      )}
      {deleting && (
        <DeleteDialog
          ids={deleting}
          onClose={(removed) => {
            setDeleting(null);
            // What was showing is gone, so nothing should still be pointing
            // at it — a stale active id leaves the Inspector describing a row
            // that no longer exists.
            if (removed > 0) activeRef.current.setActive(null);
          }}
        />
      )}
    </div>
  );
}

/** Which arm the tray goes in. The four are drawn as the cross is, so the
 * choice reads the same here as in the Inspector it is about. */
function LinkMenu({
  at,
  intoCollector,
  onPick,
  onClose,
}: {
  at: { x: number; y: number };
  /** A collector's South is what it holds — see `relate::add_to_arm`. */
  intoCollector: boolean;
  onPick: (dir: string) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [onClose]);
  const arms = [
    { key: "N", name: "North", sense: "broader" },
    { key: "W", name: "West", sense: "related" },
    { key: "E", name: "East", sense: "opposing" },
    { key: "S", name: "South", sense: intoCollector ? "inside" : "narrower" },
  ];
  return (
    <div className="link-menu-backdrop" onClick={onClose}>
      <div
        className="link-menu"
        role="menu"
        style={{ left: at.x, bottom: window.innerHeight - at.y + 8 }}
        onClick={(e) => e.stopPropagation()}
      >
        {arms.map((a) => (
          <button
            key={a.key}
            className={`link-arm at-${a.key.toLowerCase()}`}
            role="menuitem"
            data-dir={a.key}
            onClick={() => onPick(a.key)}
          >
            <b>{a.name}</b>
            <span>{a.sense}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

export default function App() {
  return (
    <ActiveItemProvider>
      <WorkbenchProvider>
        <Shell />
      </WorkbenchProvider>
    </ActiveItemProvider>
  );
}
