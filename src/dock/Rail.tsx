// The view switcher. One button per top-level destination — never one per
// projection. Miller columns, the grid layout, the board canvas and the note
// editor are all ways of looking at whatever is open, so they live inside
// "Viewer" rather than getting a rail button each; a folder's own layout
// toggle (column/grid/list) decides which of those the Viewer draws.

export type Destination = {
  kind: string;
  label: string;
  glyph: string;
  enabled: boolean;
};

export const DESTINATIONS: Destination[] = [
  { kind: "library", label: "Library", glyph: "▤", enabled: true },
  { kind: "scattered", label: "Scattered", glyph: "⁂", enabled: false },
  { kind: "viewer", label: "Viewer", glyph: "◉", enabled: false },
  { kind: "graph", label: "Graph", glyph: "❋", enabled: false },
  { kind: "discover", label: "Discover", glyph: "✦", enabled: false },
  { kind: "inspector", label: "Inspector", glyph: "◫", enabled: false },
  { kind: "compass", label: "Compass", glyph: "✛", enabled: false },
];

type Props = {
  active: string;
  onSelect: (kind: string) => void;
  onAddFolder: () => void;
};

export function Rail({ active, onSelect, onAddFolder }: Props) {
  return (
    <nav className="rail" aria-label="Views">
      <span className="rail-mark" title="Archiva">
        A
      </span>
      {DESTINATIONS.map((d) => (
        <button
          key={d.kind}
          className={"rail-btn" + (d.kind === active ? " on" : "")}
          title={d.enabled ? d.label : `${d.label} — not built yet`}
          disabled={!d.enabled}
          onClick={() => onSelect(d.kind)}
        >
          {d.glyph}
        </button>
      ))}
      <div className="rail-spacer" />
      <button className="rail-btn" title="Add a folder to the library" onClick={onAddFolder}>
        +
      </button>
    </nav>
  );
}
