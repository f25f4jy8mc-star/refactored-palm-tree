// Every app-wide shortcut, in one table.
//
// The rebuild exists because eleven views each kept their own copy of the
// same four decisions, so this is deliberately the *only* place a global
// key binding is declared. A view still owns keys that only make sense
// inside it (arrows within a list, type-ahead), but anything that works
// "wherever you are" is here, and it is one listener rather than one per
// component — two handlers both moving the selection is the exact bug
// listed in the handoff.
//
// Ported from Build 17's bindings. Where a binding's feature doesn't exist
// yet it is listed in DEFERRED rather than silently dropped, so the gap is
// visible instead of being rediscovered later.

export type ShortcutId =
  | "preview"
  | "closePanel"
  | "splitRight"
  | "splitDown"
  | "cycleGroupForward"
  | "cycleGroupBack"
  | "focusSearch"
  | "layoutGrid"
  | "layoutList"
  | "layoutColumn"
  | "selectAll"
  | "clearSelection"
  | "stepNext"
  | "stepPrev"
  | "deleteSelection";

export type Shortcut = {
  id: ShortcutId;
  /** Human-readable, for tooltips and the eventual help sheet. */
  label: string;
  keys: string;
  match: (e: KeyboardEvent) => boolean;
};

const mod = (e: KeyboardEvent) => e.metaKey || e.ctrlKey;
const bare = (e: KeyboardEvent) => !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey;

/**
 * True when the event came from somewhere that owns its own keystrokes —
 * a text field or the note editor. Bindings that would swallow typing must
 * check this; ⌘-chords generally need not.
 */
export function isTyping(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  if (t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement) return true;
  return !!t.closest?.(".cm-editor") || t.isContentEditable;
}

export const SHORTCUTS: Shortcut[] = [
  {
    id: "preview",
    label: "Quick Look the focused item",
    keys: "Space",
    match: (e) => e.code === "Space" && bare(e),
  },
  {
    id: "closePanel",
    label: "Close the active panel",
    keys: "⌘W",
    match: (e) => mod(e) && e.key.toLowerCase() === "w",
  },
  {
    id: "splitRight",
    label: "Split right",
    keys: "⌘\\",
    match: (e) => mod(e) && e.key === "\\" && !e.shiftKey,
  },
  {
    id: "splitDown",
    label: "Split down",
    keys: "⌘⇧\\",
    match: (e) => mod(e) && e.key === "\\" && e.shiftKey,
  },
  {
    id: "cycleGroupForward",
    label: "Next pane",
    keys: "⌘]",
    match: (e) => mod(e) && e.key === "]",
  },
  {
    id: "cycleGroupBack",
    label: "Previous pane",
    keys: "⌘[",
    match: (e) => mod(e) && e.key === "[",
  },
  {
    id: "focusSearch",
    label: "Search",
    keys: "⌘⇧Space",
    match: (e) => mod(e) && e.shiftKey && e.code === "Space",
  },
  {
    id: "layoutGrid",
    label: "Icon view",
    keys: "⌘1",
    match: (e) => mod(e) && e.key === "1",
  },
  {
    id: "layoutList",
    label: "List view",
    keys: "⌘2",
    match: (e) => mod(e) && e.key === "2",
  },
  {
    id: "layoutColumn",
    label: "Column view",
    keys: "⌘3",
    match: (e) => mod(e) && e.key === "3",
  },
  {
    id: "selectAll",
    label: "Select all",
    keys: "⌘A",
    match: (e) => mod(e) && e.key.toLowerCase() === "a",
  },
  {
    id: "clearSelection",
    label: "Clear selection",
    keys: "Esc",
    match: (e) => e.key === "Escape" && bare(e),
  },
  {
    id: "deleteSelection",
    label: "Remove the selection from the library",
    keys: "⌫",
    // Backspace and Delete both, because which one "deletes" is a matter of
    // which keyboard you are on.
    match: (e) => (e.key === "Backspace" || e.key === "Delete") && !mod(e) && !e.altKey,
  },
  // Build 17's fallback arrows: in a pane that owns no list of its own —
  // the Inspector, and later the note and graph views — ↑/↓ walk the order
  // the last list published, so the item under inspection can be stepped
  // without going back to the list to do it. A pane that *does* own a list
  // handles its own arrows and never sees these, which is what stops the
  // selection moving twice.
  {
    id: "stepNext",
    label: "Next item in the published order",
    keys: "↓",
    match: (e) => e.key === "ArrowDown" && !mod(e) && !e.altKey,
  },
  {
    id: "stepPrev",
    label: "Previous item in the published order",
    keys: "↑",
    match: (e) => e.key === "ArrowUp" && !mod(e) && !e.altKey,
  },
];

/** Panel kinds that own a list and therefore handle their own arrow keys. */
export const LIST_OWNING_PANES = ["library", "scattered", "viewer"] as const;

/**
 * Bindings Build 17 had whose feature doesn't exist here yet. Listed, not
 * implemented: a key that looks bound but does nothing is worse than one
 * that was never offered.
 *
 *   ⌘Z      undo            — no mutation history exists yet, which is also
 *                             why deleting asks before it acts
 *   ⌘N      create          — no create-node mutation exists
 *   ⌘⌥−     minimise bar    — the taskbar has no collapsed state yet
 */
export const DEFERRED = ["⌘Z", "⌘N", "⌘⌥−"] as const;

/**
 * Resolve an event to at most one shortcut. First match wins; the matchers
 * are written to be mutually exclusive (Space requires *no* modifiers, so
 * ⌘⇧Space can never also read as a preview), so the table's order is not
 * load-bearing — but a new binding must keep that property.
 *
 * Two things consume this, and the split matches Build 17's: `App` handles
 * the bindings that work wherever you are (preview, panel management,
 * search), and a view handles the ones that only mean something inside it
 * (layout, select-all) while it is the active pane. Both call this same
 * function, so what a key *is* can't drift between them even though where
 * it's handled differs.
 */
export function resolve(e: KeyboardEvent): ShortcutId | null {
  return SHORTCUTS.find((s) => s.match(e))?.id ?? null;
}
