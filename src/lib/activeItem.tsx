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

import { createContext, useContext, useMemo, useState } from "react";

export type ActiveItem = {
  id: string | null;
  /** Sibling ids in the publishing view's rendered order. */
  order: string[];
};

type ActiveItemApi = ActiveItem & {
  setActive: (id: string | null, order?: string[]) => void;
  step: (delta: number) => string | null;
};

const ActiveItemContext = createContext<ActiveItemApi>({
  id: null,
  order: [],
  setActive: () => {},
  step: () => null,
});

export function ActiveItemProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<ActiveItem>({ id: null, order: [] });

  // A fresh object every render, deliberately: that is what pushes updates
  // past dockview's memoised portals.
  const value: ActiveItemApi = {
    id: state.id,
    order: state.order,
    setActive: (id, order) => setState((s) => ({ id, order: order ?? s.order })),
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
