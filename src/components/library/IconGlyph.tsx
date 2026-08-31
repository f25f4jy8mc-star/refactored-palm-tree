// Renders `icon_kind`, which the indexer derives from `content_type_tree`
// (§1.6 of the model). No view keeps its own type→icon map — this is the one
// place that translates the derived kind into a mark, and it never inspects
// `content_type` itself.

const PATHS: Record<string, string> = {
  image: "M3 5h14v10H3zM3 12l4-4 3 3 4-5 3 4",
  video: "M3 5h11v10H3zM14 9l4-3v8l-4-3z",
  audio: "M6 4v9a2 2 0 1 0 0 4M6 4l8-1v9",
  document: "M5 2h7l4 4v12H5zM12 2v4h4",
  model: "M10 2l7 4v8l-7 4-7-4V6z M10 2v16 M3 6l7 4 7-4",
  note: "M4 3h12v14H4zM7 7h6M7 10h6M7 13h4",
  folder: "M3 6h5l2 2h7v8H3z",
  board: "M3 3h14v14H3zM3 8h14M8 3v14",
  tag: "M3 10l7-7h6v6l-7 7zM12 6.5a.5.5 0 1 1 0-1 .5.5 0 0 1 0 1z",
  file: "M5 2h7l4 4v12H5z",
};

export function IconGlyph({ kind }: { kind: string }) {
  const d = PATHS[kind] ?? PATHS.file;
  return (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <path d={d} fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}
