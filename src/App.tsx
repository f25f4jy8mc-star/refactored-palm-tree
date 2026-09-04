import { useCallback, useEffect, useRef, useState } from "react";

import "./App.css";
import Dock, { DockHandle, PanelKind, PanelParams } from "./dock/Dock";
import { Rail } from "./dock/Rail";
import { TaskBar } from "./dock/TaskBar";
import { ActiveItemProvider, useActiveItem } from "./lib/activeItem";
import { LIST_OWNING_PANES, isTyping, resolve } from "./lib/shortcuts";
import { LibraryView } from "./components/library/LibraryView";
import { ViewerPane } from "./components/viewer/ViewerPane";
import { InspectorView } from "./components/inspector/InspectorView";
import { PreviewOverlay } from "./components/preview/PreviewOverlay";
import { SourcesFlyout } from "./components/sources/SourcesFlyout";

function Shell() {
  const dockRef = useRef<DockHandle | null>(null);
  const [activeKind, setActiveKind] = useState<PanelKind | null>("library");
  const [previewOpen, setPreviewOpen] = useState(false);
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const active = useActiveItem();
  const activeId = active.id;
  // The provider hands out a fresh object every render on purpose (it is
  // what pushes updates through dockview's portals), so the shortcut effect
  // reads it through a ref rather than listing it as a dependency — one
  // stable listener instead of a new one on every keystroke.
  const activeRef = useRef(active);
  activeRef.current = active;

  const renderPanel = useCallback((params: PanelParams, isActive: boolean) => {
    switch (params.kind) {
      case "library":
        return (
          <LibraryView
            mode="library"
            isActive={isActive}
            onOpenCollector={(id, title) => dockRef.current?.open("viewer", title, id)}
          />
        );
      case "scattered":
        return (
          <LibraryView
            mode="scattered"
            isActive={isActive}
            onOpenCollector={(id, title) => dockRef.current?.open("viewer", title, id)}
          />
        );
      case "viewer":
        return <ViewerPane scopeId={params.scopeId} isActive={isActive} />;
      case "inspector":
        return <InspectorView isActive={isActive} />;
    }
  }, []);

  // The app-wide half of the shortcut table. View-scoped keys (⌘1/2/3,
  // ⌘A, arrows) are handled by the focused view, which resolves them
  // against the same table — see lib/shortcuts.ts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const shortcut = resolve(e);
      if (!shortcut) return;
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
  }, [activeId, previewOpen, activeKind]);

  return (
    <div className="shell" onClick={() => sourcesOpen && setSourcesOpen(false)}>
      <Rail
        activeKind={activeKind}
        sourcesOpen={sourcesOpen}
        onOpen={(kind, title) => dockRef.current?.open(kind, title)}
        onToggleSources={() => setSourcesOpen((o) => !o)}
      />
      {sourcesOpen && <SourcesFlyout onClose={() => setSourcesOpen(false)} />}
      <div className="main">
        <div className="dock-area">
          <Dock
            renderPanel={renderPanel}
            onActivePanelChange={(_title, kind) => setActiveKind(kind)}
            onReady={(handle) => (dockRef.current = handle)}
          />
        </div>
        <TaskBar />
      </div>
      {previewOpen && activeId && <PreviewOverlay onClose={() => setPreviewOpen(false)} />}
    </div>
  );
}

export default function App() {
  return (
    <ActiveItemProvider>
      <Shell />
    </ActiveItemProvider>
  );
}
