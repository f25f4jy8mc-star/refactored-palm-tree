// The docking shell. One `DockviewReact` instance hosts every pane; each
// view lands here as its own panel kind as it's converted, per the rebuild
// sequence — the shell exists before the eleventh view does, not after.
//
// Only "library" is wired today. Adding a kind later means: add it to
// `PanelKind`, teach `renderPanel` to draw it, and call `open()` — nothing
// about the docking plumbing changes.

import { createContext, useCallback, useContext, useMemo, useRef } from "react";
import {
  DockviewApi,
  DockviewReact,
  DockviewReadyEvent,
  IDockviewHeaderActionsProps,
  IDockviewPanelProps,
  themeLight,
} from "dockview-react";
import "dockview-react/dist/styles/dockview.css";

export type PanelKind = "library";

export type PanelParams = { kind: PanelKind };

export type DockHandle = {
  /** Add a panel, or focus it if one of this kind is already open. */
  open: (kind: PanelKind, title: string) => void;
};

type Props = {
  renderPanel: (kind: PanelKind) => React.ReactNode;
  onActivePanelChange: (title: string | null) => void;
  onReady: (handle: DockHandle) => void;
};

/**
 * Dockview renders each panel once, into a portal it holds in its own state.
 * A parent re-render does not reach inside that portal, so anything a panel
 * needs from outside itself has to arrive through context, not props — a
 * context consumer re-renders on its own even when the tree above it bails
 * out. `renderPanel` is handed down this way for exactly that reason.
 */
const RenderContext = createContext<(kind: PanelKind) => React.ReactNode>(() => null);

export default function Dock({ renderPanel, onActivePanelChange, onReady }: Props) {
  const apiRef = useRef<DockviewApi | null>(null);

  const components = useMemo(
    () => ({
      panel: (props: IDockviewPanelProps<PanelParams>) => {
        const render = useContext(RenderContext);
        return <div className="dock-panel">{render(props.params.kind)}</div>;
      },
    }),
    [],
  );

  const rightActions = useCallback(
    (props: IDockviewHeaderActionsProps) => (
      <div className="dock-actions">
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
    ),
    [],
  );

  const handleReady = useCallback(
    (event: DockviewReadyEvent) => {
      apiRef.current = event.api;
      event.api.onDidActivePanelChange((e) => onActivePanelChange(e.panel?.title ?? null));

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
