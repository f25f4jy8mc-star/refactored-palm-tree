import { describe, expect, it } from "vitest";

import { nodeIdOf, nodeIdsOf, placementKeys, placementsOf } from "./placement";

const rows = (spec: [string, number][]) => spec.map(([id, depth]) => ({ id, depth }));

describe("placementKeys", () => {
  it("leaves a flat list alone", () => {
    expect(placementKeys(rows([["a", 0], ["b", 0], ["c", 0]]))).toEqual(["a", "b", "c"]);
  });

  it("separates the two rows one item gets when its folder is expanded", () => {
    // The reported bug: `photo` is listed at the top level and again inside
    // the folder that contains it. Same id, two rows.
    const keys = placementKeys(rows([["photo", 0], ["trips", 0], ["photo", 1]]));
    expect(keys).toEqual(["photo", "trips", "trips>photo"]);
    expect(new Set(keys).size).toBe(3);
  });

  it("gives one item different keys under different parents", () => {
    const keys = placementKeys(
      rows([["trips", 0], ["photo", 1], ["work", 0], ["photo", 1]]),
    );
    expect(keys).toEqual(["trips", "trips>photo", "work", "work>photo"]);
  });

  it("keeps keys stable when the list is refetched unchanged", () => {
    const spec: [string, number][] = [["trips", 0], ["photo", 1], ["other", 0]];
    expect(placementKeys(rows(spec))).toEqual(placementKeys(rows(spec)));
  });

  it("is not positional, so inserting a row above does not rename the ones below", () => {
    const before = placementKeys(rows([["trips", 0], ["photo", 1]]));
    const after = placementKeys(rows([["new", 0], ["trips", 0], ["photo", 1]]));
    expect(after.slice(1)).toEqual(before);
  });

  it("closes a branch when the list comes back up a level", () => {
    const keys = placementKeys(
      rows([["a", 0], ["a1", 1], ["a1a", 2], ["b", 0], ["b1", 1]]),
    );
    expect(keys).toEqual(["a", "a>a1", "a>a1>a1a", "b", "b>b1"]);
  });

  it("treats a child with no parent above it as top level rather than throwing", () => {
    expect(placementKeys(rows([["orphan", 1]]))).toEqual(["orphan"]);
  });
});

describe("nodeIdOf", () => {
  it("reads the item out of a placement", () => {
    expect(nodeIdOf("trips>photo")).toBe("photo");
    expect(nodeIdOf("photo")).toBe("photo");
    expect(nodeIdOf("a>b>c")).toBe("c");
  });
});

describe("nodeIdsOf", () => {
  it("counts an item selected in two places once", () => {
    expect(nodeIdsOf(["photo", "trips>photo", "other"])).toEqual(["photo", "other"]);
  });

  it("keeps the order it was given", () => {
    expect(nodeIdsOf(["b", "a", "trips>b"])).toEqual(["b", "a"]);
  });

  it("copes with an empty selection", () => {
    expect(nodeIdsOf([])).toEqual([]);
  });
});

describe("placementsOf", () => {
  it("finds every row showing one item", () => {
    const keys = ["photo", "trips", "trips>photo", "other"];
    expect(placementsOf(keys, "photo")).toEqual(["photo", "trips>photo"]);
    expect(placementsOf(keys, "missing")).toEqual([]);
  });

  it("does not match an item whose id is a suffix of another", () => {
    // "photo" must not match "myphoto" — the separator is part of the test.
    expect(placementsOf(["trips>myphoto"], "photo")).toEqual([]);
  });
});
