// What the taskbar offers, decided in one place from what is on screen.
//
// The bar has two tiers. The upper one belongs to the focused view — its
// layout, sort and filter — and each view portals its own controls there.
// This module decides the lower one: the actions that act on *things* — make
// something, tag the selection, stage it in the tray, put the tray somewhere.
// Which of those make sense depends on which pane is active, what it has
// selected and what the tray holds, and that dependency is written here as a
// pure function rather than as conditionals scattered through a component, so
// it can be tested without a browser and so two surfaces offering the same
// action cannot disagree about when it applies.
//
// Two kinds of absence, deliberately different:
//
//   * An action that always exists but cannot act right now (Tag with nothing
//     selected) is drawn **disabled**, with a title saying why. It is where
//     you expect it, and the reason is the prompt.
//   * An action that only exists in a context (Add to this folder, Link to
//     this item) is **not drawn** outside it. A disabled "Add 3 to —" would be
//     a button about nothing.

import type { PanelKind } from "../dock/Dock";
import type { GatherTarget } from "./types";

export type BarContext = {
  /** The active pane's kind, or null before any pane has focus. */
  pane: PanelKind | null;
  /** Ids selected in the view that last published a selection. */
  selection: string[];
  tray: string[];
  /** The collector the active Viewer shows the inside of, when it can take
   * members. Null for the library root, and for a folder mirrored from disk. */
  scope: GatherTarget | null;
  /** The item the Inspector is showing, when the Inspector is the pane. */
  inspecting: { id: string; name: string } | null;
};

export type ActionId =
  | "new"
  | "tag"
  | "toTray"
  | "inspect"
  | "delete"
  | "gather"
  | "link";

export type BarAction = {
  id: ActionId;
  label: string;
  /** Tooltip: what it will do, or why it cannot. */
  title: string;
  enabled: boolean;
  /** The one thing most worth doing next, drawn emphasised. At most one. */
  primary?: boolean;
  /** Keyboard hint, when the action has one. */
  keys?: string;
};

const LISTS: readonly PanelKind[] = ["library", "scattered", "viewer"];

const n = (count: number, one: string, many = `${one}s`) =>
  `${count} ${count === 1 ? one : many}`;

export function barActions(ctx: BarContext): BarAction[] {
  const out: BarAction[] = [];
  const sel = ctx.selection.length;
  const inList = ctx.pane !== null && LISTS.includes(ctx.pane);

  // Making something. Inside a collector you made, what you make goes in it —
  // the label says so, so where a new note lands is never a surprise.
  out.push({
    id: "new",
    label: ctx.scope ? `＋ New in ${ctx.scope.name}` : "＋ New",
    title: ctx.scope
      ? `Make a note, folder, board or link inside ${ctx.scope.name}`
      : "Make a note, folder, board or link",
    enabled: true,
    keys: "⌘N",
  });

  // Tagging is a batch operation (C2), always offered, because "tag this" is
  // the most common thing to want and its absence would be a hole in the bar.
  out.push({
    id: "tag",
    label: sel > 1 ? `# Tag ${sel}` : "# Tag",
    title: sel === 0 ? "Select something to tag" : `Tag ${n(sel, "item")}`,
    enabled: sel > 0,
    keys: "⌘T",
  });

  if (sel > 0) {
    const fresh = ctx.selection.filter((id) => !ctx.tray.includes(id)).length;
    if (fresh > 0) {
      out.push({
        id: "toTray",
        label: fresh > 1 ? `⊕ ${fresh} to tray` : "⊕ Tray",
        title: `Hold ${n(fresh, "item")} in the tray, to put somewhere in one go`,
        enabled: true,
      });
    }
    if (ctx.pane !== "inspector") {
      out.push({
        id: "inspect",
        label: "◫ Inspect",
        title: "Open the Inspector on what is selected",
        enabled: true,
      });
    }
    if (inList) {
      out.push({
        id: "delete",
        label: sel > 1 ? `Delete ${sel}` : "Delete",
        title: `Remove ${n(sel, "item")} from the library — you are asked first`,
        enabled: true,
        keys: "⌫",
      });
    }
  }

  // What the tray is for: putting what it holds somewhere. The destination is
  // whatever the active pane is about, so the offer follows focus.
  if (ctx.tray.length > 0) {
    if (ctx.pane === "viewer" && ctx.scope) {
      const count = ctx.tray.filter((id) => id !== ctx.scope?.id).length;
      if (count > 0) {
        out.push({
          id: "gather",
          label: `Add ${count} to ${ctx.scope.name}`,
          title: `Put the tray's ${n(count, "item")} in this ${ctx.scope.kind}`,
          enabled: true,
          primary: true,
        });
      }
    }
    if (ctx.pane === "inspector" && ctx.inspecting) {
      const count = ctx.tray.filter((id) => id !== ctx.inspecting?.id).length;
      if (count > 0) {
        out.push({
          id: "link",
          label: `Link ${count} to ${ctx.inspecting.name}…`,
          title: `Choose a compass direction for the tray's ${n(count, "item")}`,
          enabled: true,
          primary: true,
        });
      }
    }
  }
  return out;
}

/** What the bar says about the selection, beside the actions. */
export function barStatus(ctx: BarContext): string {
  const sel = ctx.selection.length;
  if (sel === 0) return "Nothing selected";
  return `${n(sel, "item")} selected`;
}
