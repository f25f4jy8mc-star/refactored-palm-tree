// What the Inspector is inspecting and what Space previews: one active
// item, published by whichever view the user is working in.
//
// Two things make this the shape it is:
//
//   * It travels by **context**, because a panel lives inside a dockview
//     portal that a parent re-render doesn't reach. Same reason `Dock`
//     passes its renderer down this way.
//   * It carries the **order it came from**, not just an id (G16). Preview
//     steps ←/→ through siblings "in the order of the list it was opened
//     from", and a copied array would go stale the moment the underlying
//     list changed — so views republish this whenever their order changes,
//     and Preview reads the current one rather than a snapshot.

import { createContext, useCallback, useContext, useMemo, useState } from "react";

export type ActiveItem = {
  id: string | null;
  /** Sibling ids in the publishing view's rendered order. */
  order: string[];
  /** Everything currently selected in the publishing view, in that same
   * order. Carried because tagging is a batch operation (C2): the Inspector
   * shows one item but applies to the whole selection, and it has no other
   * way to know what that is. Always contains the active item, so a caller
   * can use it without checking for the empty case. */
  selection: string[];
};

type ActiveItemApi = ActiveItem & {
  setActive: (id: string | null, order?: string[]) => void;
  setSelection: (ids: string[]) => void;
  step: (delta: number) => string | null;
};

const ActiveItemContext = createContext<ActiveItemApi>({
  id: null,
  order: [],
  selection: [],
  setActive: () => {},
  setSelection: () => {},
  step: () => null,
});

export function ActiveItemProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<ActiveItem>({ id: null, order: [], selection: [] });

  // Stable identities, because a view publishes its selection from an effect
  // and an unstable setter there is an infinite loop: the effect's own
  // dependency would change on every render it caused.
  //
  // Making an item active with no explicit selection *is* a selection of one.
  // Leaving the previous selection standing is how a batch edit ends up
  // applied to whatever happened to be highlighted two clicks ago.
  const setActive = useCallback<ActiveItemApi["setActive"]>(
    (id, order) =>
      setState((s) => ({
        id,
        order: order ?? s.order,
        selection: id ? [id] : [],
      })),
    [],
  );

  const setSelection = useCallback<ActiveItemApi["setSelection"]>(
    (ids) =>
      setState((s) => {
        // The active item is always part of its own selection, so callers
        // never have to special-case it.
        const next = s.id && !ids.includes(s.id) ? [s.id, ...ids] : ids;
        const same =
          next.length === s.selection.length &&
          next.every((x, i) => x === s.selection[i]);
        // Bail on an unchanged list: a publishing effect re-runs whenever its
        // view re-renders, and returning a fresh array every time would keep
        // every consumer re-rendering forever.
        return same ? s : { ...s, selection: next };
      }),
    [],
  );

  // A fresh object every render, deliberately: that is what pushes updates
  // past dockview's memoised portals.
  const value: ActiveItemApi = {
    id: state.id,
    order: state.order,
    selection: state.selection,
    setActive,
    setSelection,
    step: (delta) => {
      const { id, order } = state;
      if (!id || order.length === 0) return null;
      const i = order.indexOf(id);
      if (i === -1) return null;
      // Wraps, as Preview's ←/→ did in the old build.
      const next = (i + delta + order.length) % order.length;
      return order[next];
    },
  };

  return <ActiveItemContext.Provider value={value}>{children}</ActiveItemContext.Provider>;
}

export function useActiveItem(): ActiveItemApi {
  return useContext(ActiveItemContext);
}

/** Stable identity for the publishing side, so a view can hand its order
 * up without re-running effects on every keystroke. */
export function useOrderPublisher(order: string[]) {
  const { setActive } = useActiveItem();
  return useMemo(() => ({ publish: (id: string | null) => setActive(id, order) }), [order, setActive]);
}
