// The floating bottom bar. It owns no view-specific logic at all — every
// panel portals its own filter/sort/layout controls into the shared slot
// when (and only when) it's the active pane, so the bar always shows
// exactly the focused view's own controls without the bar needing to know
// what a "kind filter" or a "layout toggle" is.
//
// The slot is looked up in an effect rather than at render time: it lives in
// a sibling that mounts after this component, so a synchronous lookup during
// render would find nothing.

import { useEffect, useState } from "react";

export const TASKBAR_SLOT_ID = "taskbar-slot";

export function useTaskbarSlot(id: string = TASKBAR_SLOT_ID): HTMLElement | null {
  const [el, setEl] = useState<HTMLElement | null>(null);
  useEffect(() => {
    setEl(document.getElementById(id));
  }, [id]);
  return el;
}

export function TaskBar() {
  return (
    <div className="taskbar">
      <div className="taskbar-row" id={TASKBAR_SLOT_ID} />
    </div>
  );
}
