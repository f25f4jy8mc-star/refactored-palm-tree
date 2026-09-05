import { describe, expect, it } from "vitest";

import { isCollapsed, sectionsOf, toggleSection, visibleRows } from "./sections";

const rows = (spec: [string, string][]) =>
  spec.map(([group_key, group_label]) => ({ group_key, group_label }));

describe("sectionsOf", () => {
  it("finds each section once, in the order the rows arrived", () => {
    expect(
      sectionsOf(
        rows([
          ["image", "Images"],
          ["image", "Images"],
          ["note", "Notes"],
        ]),
      ),
    ).toEqual([
      { key: "image", label: "Images", count: 2 },
      { key: "note", label: "Notes", count: 1 },
    ]);
  });

  it("counts a group that comes back rather than drawing it twice", () => {
    const out = sectionsOf(
      rows([
        ["a", "A"],
        ["b", "B"],
        ["a", "A"],
      ]),
    );
    expect(out.map((s) => s.key)).toEqual(["a", "b"]);
    expect(out[0].count).toBe(2);
  });

  it("copes with an empty list", () => {
    expect(sectionsOf([])).toEqual([]);
  });
});

describe("visibleRows", () => {
  it("keeps everything when nothing is folded", () => {
    const all = rows([
      ["a", "A"],
      ["b", "B"],
    ]);
    expect(visibleRows(all, [])).toBe(all);
  });

  it("drops the rows of a folded section only", () => {
    const all = rows([
      ["a", "A"],
      ["b", "B"],
      ["a", "A"],
    ]);
    expect(visibleRows(all, ["a"])).toEqual([{ group_key: "b", group_label: "B" }]);
  });

  it("can fold everything", () => {
    const all = rows([
      ["a", "A"],
      ["b", "B"],
    ]);
    expect(visibleRows(all, ["a", "b"])).toEqual([]);
  });

  it("still reports the sections when their rows are hidden", () => {
    // The header has to survive, or there is nothing left to click.
    const all = rows([
      ["a", "A"],
      ["b", "B"],
    ]);
    expect(sectionsOf(all).map((s) => s.key)).toEqual(["a", "b"]);
    expect(visibleRows(all, ["a", "b"])).toEqual([]);
  });
});

describe("toggleSection", () => {
  it("folds and unfolds", () => {
    expect(toggleSection([], "a")).toEqual(["a"]);
    expect(toggleSection(["a"], "a")).toEqual([]);
    expect(toggleSection(["a"], "b")).toEqual(["a", "b"]);
  });

  it("answers whether one is folded", () => {
    expect(isCollapsed(["a"], "a")).toBe(true);
    expect(isCollapsed(["a"], "b")).toBe(false);
  });
});
