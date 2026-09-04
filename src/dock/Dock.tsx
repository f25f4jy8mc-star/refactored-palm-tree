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

export type PanelKind = "library" | "scattered" | "viewer" | "inspector";

/** `scopeId` is which collector a Viewer pane is showing — `undefined` is
 * the library root. It rides on the panel so a split or a reopened layout
 * keeps showing what it was showing. */
export type PanelParams = { kind: PanelKind; scopeId?: string };

export type DockHandle = {
  /** Add a panel, or focus it if one of this kind is already open. A
   * `scopeId` retargets an existing panel of that kind rather than piling
   * up a second one — the old build's "retargetViewer", which is what
   * stops every double-click opening another tab. */
  open: (kind: PanelKind, title: string, scopeId?: string) => void;
  /** Close the active panel. */
  closeActive: () => void;
  /** Split the active panel's group, right or below. */
  split: (direction: "right" | "below") => void;
  /** Move focus between panes. */
  cycleGroup: (delta: number) => void;
};

type RenderPanel = (params: PanelParams, isActive: boolean) => React.ReactNode;

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
        // `params` is re-read on change too: retargeting a Viewer at a
        // different collector updates the panel in place rather than
        // replacing it, so the change has to reach the renderer.
        const [params, setParams] = useState<PanelParams>(props.params);
        useEffect(() => {
          setActive(props.api.isActive);
          setParams(props.params);
          const a = props.api.onDidActiveChange((e) => setActive(e.isActive));
          const p = props.api.onDidParametersChange((next) => setParams(next as PanelParams));
          return () => {
            a.dispose();
            p.dispose();
          };
        }, [props.api, props.params]);
        return <div className="dock-panel">{render(params, active)}</div>;
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

      const open: DockHandle["open"] = (kind, title, scopeId) => {
        // Prefer a panel of this kind that's already open — including one
        // created by a split, whose id is suffixed. Retargeting it is what
        // keeps double-clicking a folder from piling up tabs.
        const existing =
          event.api.getPanel(kind) ??
          event.api.panels.find((p) => (p.params as PanelParams | undefined)?.kind === kind);
        if (existing) {
          if (scopeId !== undefined) {
            existing.api.updateParameters({ kind, scopeId });
            existing.api.setTitle(title);
          }
          existing.api.setActive();
          return;
        }
        event.api.addPanel<PanelParams>({
          id: kind,
          component: "panel",
          title,
          params: { kind, scopeId },
        });
      };

      const closeActive: DockHandle["closeActive"] = () => {
        event.api.activePanel?.api.close();
      };

      const split: DockHandle["split"] = (direction) => {
        const active = event.api.activePanel;
        const group = active?.group ?? event.api.activeGroup;
        if (!group) return;
        const params = (active?.params as PanelParams | undefined) ?? { kind: "library" };
        event.api.addPanel<PanelParams>({
          id: `${params.kind}~${Date.now()}`,
          component: "panel",
          title: active?.title ?? "Library",
          params,
          position: { referenceGroup: group, direction },
        });
      };

      const cycleGroup: DockHandle["cycleGroup"] = (delta) => {
        const groups = event.api.groups;
        if (groups.length < 2) return;
        const i = groups.findIndex((g) => g.id === event.api.activeGroup?.id);
        groups[(i + delta + groups.length) % groups.length].api.setActive();
      };

      onReady({ open, closeActive, split, cycleGroup });
      // Library opens by default so the shell is never staring at an empty
      // grid.
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
