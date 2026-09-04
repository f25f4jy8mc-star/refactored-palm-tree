// The docking shell. One `DockviewReact` instance hosts every pane; each
// view lands here as its own panel kind as it's converted, per the rebuild
// sequence — the shell exists before the eleventh view does, not after.
//
// Only "library" is wired today. Adding a kind later means: add it to
// `PanelKind`, teach `renderPanel` to draw it, and call `open()` — nothing
// about the docking plumbing changes.

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import {
  DockviewApi,
  DockviewReact,
  DockviewReadyEvent,
  IDockviewHeaderActionsProps,
  IDockviewPanelProps,
  themeLight,
} from "dockview-react";
import "dockview-react/dist/styles/dockview.css";

export type PanelKind = "library" | "scattered" | "viewer";

export type PanelParams = { kind: PanelKind };

export type DockHandle = {
  /** Add a panel, or focus it if one of this kind is already open. */
  open: (kind: PanelKind, title: string) => void;
};

type RenderPanel = (kind: PanelKind, isActive: boolean) => React.ReactNode;

/**
 * Dockview renders each panel once, into a portal it holds in its own state.
 * A parent re-render does not reach inside that portal, so anything a panel
 * needs from outside itself has to arrive through context, not props — a
 * context consumer re-renders on its own even when the tree above it bails
 * out. `renderPanel` is handed down this way for exactly that reason.
 */
const RenderContext = createContext<RenderPanel>(() => null);

export default function Dock({ renderPanel, onActivePanelChange, onReady }: {
  renderPanel: RenderPanel;
  onActivePanelChange: (title: string | null, kind: PanelKind | null) => void;
  onReady: (handle: DockHandle) => void;
}) {
  const apiRef = useRef<DockviewApi | null>(null);

  const components = useMemo(
    () => ({
      panel: (props: IDockviewPanelProps<PanelParams>) => {
        const render = useContext(RenderContext);
        // Only the taskbar cares about this (a panel portals its controls
        // into the shared slot only while active), so it isn't threaded
        // through onActivePanelChange, which is a title string for the
        // window chrome.
        const [active, setActive] = useState(props.api.isActive);
        useEffect(() => {
          setActive(props.api.isActive);
          const d = props.api.onDidActiveChange((e) => setActive(e.isActive));
          return () => d.dispose();
        }, [props.api]);
        return <div className="dock-panel">{render(props.params.kind, active)}</div>;
      },
    }),
    [],
  );

  const rightActions = useCallback(
    (props: IDockviewHeaderActionsProps) => {
      const split = (direction: "right" | "below") => {
        const active = props.activePanel;
        const kind = (active?.params as PanelParams | undefined)?.kind ?? "library";
        const title = active?.title ?? "Library";
        props.containerApi.addPanel<PanelParams>({
          id: `${kind}~${Date.now()}`,
          component: "panel",
          title,
          params: { kind },
          position: { referenceGroup: props.group, direction },
        });
      };
      return (
        <div className="dock-actions">
          <button className="dock-action" title="Split right" onClick={() => split("right")}>
            ⫲
          </button>
          <button className="dock-action" title="Split down" onClick={() => split("below")}>
            ⫤
          </button>
          <button
            className="dock-action"
            title="Maximise / restore"
            onClick={() =>
              props.api.isMaximized() ? props.api.exitMaximized() : props.api.maximize()
            }
          >
            ⤢
          </button>
        </div>
      );
    },
    [],
  );

  const handleReady = useCallback(
    (event: DockviewReadyEvent) => {
      apiRef.current = event.api;
      event.api.onDidActivePanelChange((e) =>
        onActivePanelChange(e.panel?.title ?? null, (e.panel?.params as PanelParams | undefined)?.kind ?? null),
      );

      const open: DockHandle["open"] = (kind, title) => {
        const existing = event.api.getPanel(kind);
        if (existing) {
          existing.api.setActive();
          return;
        }
        event.api.addPanel<PanelParams>({
          id: kind,
          component: "panel",
          title,
          params: { kind },
        });
      };

      onReady({ open });
      // Library is the one view that exists — open it by default so the
      // shell is never staring at an empty grid.
      open("library", "Library");
    },
    [onActivePanelChange, onReady],
  );

  return (
    <RenderContext.Provider value={renderPanel}>
      <DockviewReact
        className="dock-root"
        theme={themeLight}
        components={components}
        rightHeaderActionsComponent={rightActions}
        onReady={handleReady}
      />
    </RenderContext.Provider>
  );
}
