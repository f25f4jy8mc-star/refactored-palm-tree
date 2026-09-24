// The floating bottom bar, in two tiers.
//
// The upper tier belongs to the focused view. It owns no view-specific logic
// at all — every panel portals its own filter/sort/layout controls into the
// shared slot when (and only when) it is the active pane, so the bar shows
// exactly the focused view's own controls without knowing what a "kind
// filter" or a "layout toggle" is.
//
// The lower tier acts on things: make, tag, stage, put. Which of those it
// offers is decided by `lib/taskbarActions` from what is on screen — this
// component only draws the answer — and it ends in the tray.
//
// The slot is looked up in an effect rather than at render time: it lives in
// a sibling that mounts after this component, so a synchronous lookup during
// render would find nothing.
//
// Each tier scrolls on its own; the bar itself does not. A popover opened
// from a scrolling strip is clipped by it — the lesson Build 17 recorded as
// "popups live outside the scrolling strip".

import { useEffect, useState } from "react";

import type { ActionId, BarAction } from "../lib/taskbarActions";
import { Tray } from "./Tray";

export const TASKBAR_SLOT_ID = "taskbar-slot";

export function useTaskbarSlot(id: string = TASKBAR_SLOT_ID): HTMLElement | null {
  const [el, setEl] = useState<HTMLElement | null>(null);
  useEffect(() => {
    setEl(document.getElementById(id));
  }, [id]);
  return el;
}

type Props = {
  actions: BarAction[];
  status: string;
  onAction: (id: ActionId, anchor: DOMRect) => void;
  /** An item dropped from the tray onto something the bar knows about. */
  onTrayOpen: (id: string) => void;
};

export function TaskBar({ actions, status, onAction, onTrayOpen }: Props) {
  return (
    <div className="taskbar">
      <div className="taskbar-row taskbar-view" id={TASKBAR_SLOT_ID} />
      <div className="taskbar-row taskbar-things">
        {actions.map((a) => (
          <button
            key={a.id}
            className={"btn" + (a.primary ? " primary" : "")}
            data-action={a.id}
            disabled={!a.enabled}
            title={a.keys ? `${a.title} (${a.keys})` : a.title}
            onMouseDown={(e) => e.preventDefault()}
            onClick={(e) => onAction(a.id, e.currentTarget.getBoundingClientRect())}
          >
            {a.label}
          </button>
        ))}
        <span className="taskbar-status taskbar-selection">{status}</span>
        <span className="taskbar-spacer" />
        <Tray onOpen={onTrayOpen} />
      </div>
    </div>
  );
}
