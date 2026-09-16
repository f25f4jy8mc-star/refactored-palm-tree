import { describe, expect, it } from "vitest";

import { openDestinations, openTarget, previewSource } from "./capabilities";

const media = (caps: string[] = ["preview", "full_res", "link", "tag"]) => ({
  node_type: "media",
  capabilities: caps,
});
const folder = { node_type: "collector", capabilities: ["expand", "contain", "link", "tag"] };
const board = { node_type: "collector", capabilities: ["link", "tag"] };
const tag = { node_type: "tag", capabilities: [] as string[] };

describe("openTarget", () => {
  it("takes the most particular renderer, not the first capability", () => {
    expect(openTarget({ capabilities: ["preview", "play"] })).toBe("play");
    expect(openTarget({ capabilities: ["preview"] })).toBe("preview");
    expect(openTarget({ capabilities: ["tag", "rename"] })).toBe(null);
  });
});

describe("previewSource", () => {
  const parts = {
    capabilities: ["preview", "full_res"],
    locator: "/photos/a.jpg",
    previewRef: "/proxies/a-preview.jpg",
    thumbRef: "/proxies/a-thumb.jpg",
  };

  it("takes the original when the registry says it is reachable", () => {
    expect(previewSource(parts)).toBe("/photos/a.jpg");
  });

  it("falls to the preview render when it is not", () => {
    // `full_res` is withheld for an item whose file is missing, so this is
    // the case of a photograph on an unplugged drive with a proxy on hand.
    expect(previewSource({ ...parts, capabilities: ["preview"] })).toBe("/proxies/a-preview.jpg");
  });

  it("then to the grid thumbnail", () => {
    expect(previewSource({ ...parts, capabilities: ["preview"], previewRef: null })).toBe(
      "/proxies/a-thumb.jpg",
    );
  });

  it("then to the original, which may be all there is before proxies run", () => {
    expect(
      previewSource({ ...parts, capabilities: [], previewRef: null, thumbRef: null }),
    ).toBe("/photos/a.jpg");
  });

  it("is null when there is nothing to draw", () => {
    expect(
      previewSource({ capabilities: [], locator: null, previewRef: null, thumbRef: null }),
    ).toBe(null);
  });
});

describe("openDestinations", () => {
  it("offers a folder its own Viewer", () => {
    const where = openDestinations(folder).map((o) => o.destination);
    expect(where).toEqual(["viewer", "library", "graph"]);
  });

  it("does not offer a Viewer for something with no contents", () => {
    // A photograph contains nothing, and neither does a board — `expand` is
    // the grant that says otherwise, and the model withholds it from both.
    expect(openDestinations(media()).map((o) => o.destination)).toEqual(["library", "graph"]);
    expect(openDestinations(board).map((o) => o.destination)).toEqual(["library", "graph"]);
  });

  it("offers the Graph but says it is not built", () => {
    const graph = openDestinations(media()).find((o) => o.destination === "graph");
    expect(graph?.enabled).toBe(false);
    expect(graph?.note).toBe("not built yet");
  });

  it("leaves the vocabulary out of the Library", () => {
    expect(openDestinations(tag).map((o) => o.destination)).toEqual([]);
  });

  it("offers nothing it cannot justify from the resolved list", () => {
    // An item the registry grants nothing for — a remote URL not fetched —
    // is still findable in the Library and nowhere else.
    expect(openDestinations({ node_type: "media", capabilities: [] }).map((o) => o.destination)).toEqual([
      "library",
    ]);
  });
});
