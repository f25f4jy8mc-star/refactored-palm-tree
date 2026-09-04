import type { PanelKind } from "./Dock";

// The view switcher. One button per top-level destination — never one per
// projection. Miller columns, the grid layout, the board canvas and the note
// editor are all ways of looking at whatever is open, so they live inside
// "Viewer" rather than getting a rail button each; a folder's own layout
// toggle (list/grid, in the taskbar) decides which of those the Viewer draws.
//
// Library and Scattered are the same underlying view over the same
// projection (p_rows) with a different default grouping — see
// components/library/LibraryView — so both open the same component.

type Destination = {
  kind: PanelKind | null;
  label: string;
  glyph: string;
};

const DESTINATIONS: Destination[] = [
  { kind: "library", label: "Library", glyph: "▤" },
  { kind: "scattered", label: "Scattered", glyph: "⁂" },
  { kind: "viewer", label: "Viewer", glyph: "◉" },
  { kind: null, label: "Graph", glyph: "❋" },
  { kind: null, label: "Discover", glyph: "✦" },
  { kind: null, label: "Inspector", glyph: "◫" },
  { kind: null, label: "Compass", glyph: "✛" },
];

type Props = {
  activeKind: PanelKind | null;
  onOpen: (kind: PanelKind, title: string) => void;
};

export function Rail({ activeKind, onOpen }: Props) {
  return (
    <nav className="rail" aria-label="Views">
      <span className="rail-mark" title="Archiva">
        A
      </span>
      {DESTINATIONS.map((d) => (
        <button
          key={d.label}
          className={"rail-btn" + (d.kind === activeKind ? " on" : "")}
          title={d.kind ? d.label : `${d.label} — not built yet`}
          disabled={!d.kind}
          onClick={() => d.kind && onOpen(d.kind, d.label)}
        >
          {d.glyph}
        </button>
      ))}
    </nav>
  );
}
