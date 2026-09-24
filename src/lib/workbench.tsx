// What the shell holds between views: the tray, and which collector the
// active pane is looking inside.
//
// The tray is Build 17's staging area — gather things from anywhere, then put
// them somewhere in one gesture. It holds **ids only**. Names and thumbnails
// are asked of the backend when it is drawn (`rowsOf`), so an item renamed or
// removed while it sits in the tray is drawn as it now is, or not at all,
// rather than as a copy taken when it was added.
//
// It lives in the window, not the database. It is a hand you are holding
// things in, not a fact about the library, and nothing else reads it.
//
// The scope is published by the active Viewer because only it knows which
// collector it is showing; the taskbar needs to know in order to offer "Add to
// this". Travelling by context for the same reason the active item does: a
// panel lives in a dockview portal that a parent re-render does not reach.

import { createContext, useCallback, useContext, useMemo, useState } from "react";

type Workbench = {
  tray: string[];
  /** Add to the end, skipping anything already there. */
  addToTray: (ids: string[]) => void;
  removeFromTray: (id: string) => void;
  clearTray: () => void;
  /** Drop ids that were asked about and did not come back — gone from the
   * library. Only those: an id added while the question was in flight was
   * not asked about, and a late answer must not take it out. */
  dropMissing: (asked: string[], existing: string[]) => void;
  /** The collector the active pane shows the inside of, or null. */
  paneScope: string | null;
  setPaneScope: (id: string | null) => void;
};

const Ctx = createContext<Workbench>({
  tray: [],
  addToTray: () => {},
  removeFromTray: () => {},
  clearTray: () => {},
  dropMissing: () => {},
  paneScope: null,
  setPaneScope: () => {},
});

export function WorkbenchProvider({ children }: { children: React.ReactNode }) {
  const [tray, setTray] = useState<string[]>([]);
  const [paneScope, setPaneScope] = useState<string | null>(null);

  const addToTray = useCallback(
    (ids: string[]) =>
      setTray((t) => {
        const fresh = ids.filter((id, i) => !t.includes(id) && ids.indexOf(id) === i);
        return fresh.length ? [...t, ...fresh] : t;
      }),
    [],
  );
  const removeFromTray = useCallback((id: string) => setTray((t) => t.filter((x) => x !== id)), []);
  const clearTray = useCallback(() => setTray([]), []);
  const dropMissing = useCallback(
    (asked: string[], existing: string[]) =>
      setTray((t) => {
        const gone = asked.filter((id) => !existing.includes(id));
        if (gone.length === 0) return t;
        return t.filter((id) => !gone.includes(id));
      }),
    [],
  );

  const value = useMemo(
    () => ({ tray, addToTray, removeFromTray, clearTray, dropMissing, paneScope, setPaneScope }),
    [tray, addToTray, removeFromTray, clearTray, dropMissing, paneScope],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useWorkbench(): Workbench {
  return useContext(Ctx);
}
