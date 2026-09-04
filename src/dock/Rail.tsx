import type { PanelKind } from "./Dock";

// The view switcher. One button per top-level destination — never one per
// projection. Miller columns, the icon grid and the list are three ways of
// reading one collector, so they live *inside* Viewer as a layout toggle
// (⌘1/⌘2/⌘3), not as three rail buttons.
//
// Library and Scattered are likewise the same underlying view over the
// same projection (p_rows) with a different default grouping.

type Destination = {
  kind: PanelKind | null;
  label: string;
  glyph: string;
};

const DESTINATIONS: Destination[] = [
  { kind: "library", label: "Library", glyph: "▤" },
  { kind: "scattered", label: "Scattered", glyph: "⁂" },
  { kind: "viewer", label: "Viewer", glyph: "◉" },
  { kind: "inspector", label: "Inspector", glyph: "◫" },
  { kind: null, label: "Graph", glyph: "❋" },
  { kind: null, label: "Discover", glyph: "✦" },
  { kind: null, label: "Compass", glyph: "✛" },
];

type Props = {
  activeKind: PanelKind | null;
  sourcesOpen: boolean;
  onOpen: (kind: PanelKind, title: string) => void;
  onToggleSources: () => void;
};

export function Rail({ activeKind, sourcesOpen, onOpen, onToggleSources }: Props) {
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
      <div className="rail-spacer" />
      <button
        className={"rail-btn" + (sourcesOpen ? " on" : "")}
        title="Sources — watched folders"
        onClick={(e) => {
          // The shell closes the flyout on any click; this one opened it.
          e.stopPropagation();
          onToggleSources();
        }}
      >
        ⌸
      </button>
    </nav>
  );
}
