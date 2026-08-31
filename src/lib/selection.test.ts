import { describe, expect, it } from "vitest";
import {
  EMPTY_SELECTION,
  click,
  clear,
  isSelected,
  marquee,
  moveCursor,
  rangeClick,
  selectAll,
  toggleClick,
  typeAhead,
} from "./selection";

const order = ["a", "b", "c", "d", "e"];

describe("click", () => {
  it("replaces the selection with exactly one row", () => {
    const s = click("b");
    expect([...s.ids]).toEqual(["b"]);
    expect(s.anchor).toBe("b");
    expect(s.cursor).toBe("b");
  });
});

describe("toggleClick", () => {
  it("adds an unselected row and moves the anchor to it", () => {
    const s = toggleClick(click("a"), "c");
    expect(isSelected(s, "a")).toBe(true);
    expect(isSelected(s, "c")).toBe(true);
    expect(s.anchor).toBe("c");
  });

  it("removes an already-selected row", () => {
    const s = toggleClick(click("a"), "a");
    expect(isSelected(s, "a")).toBe(false);
    expect(s.ids.size).toBe(0);
  });
});

describe("rangeClick", () => {
  it("selects the contiguous run between the anchor and the clicked row", () => {
    const s = rangeClick(click("b"), "d", order);
    expect([...s.ids].sort()).toEqual(["b", "c", "d"]);
  });

  it("keeps the same anchor across repeated shift-clicks, growing the range", () => {
    let s = click("b");
    s = rangeClick(s, "d", order);
    s = rangeClick(s, "e", order);
    expect(s.anchor).toBe("b");
    expect([...s.ids].sort()).toEqual(["b", "c", "d", "e"]);
  });

  it("shrinks back toward the anchor when the next click lands closer", () => {
    let s = click("b");
    s = rangeClick(s, "e", order);
    s = rangeClick(s, "c", order);
    expect(s.anchor).toBe("b");
    expect([...s.ids].sort()).toEqual(["b", "c"]);
  });

  it("works backwards from the anchor too", () => {
    const s = rangeClick(click("d"), "a", order);
    expect([...s.ids].sort()).toEqual(["a", "b", "c", "d"]);
  });
});

describe("selectAll", () => {
  it("selects every row and anchors at the ends", () => {
    const s = selectAll(order);
    expect(s.ids.size).toBe(order.length);
    expect(s.anchor).toBe("a");
    expect(s.cursor).toBe("e");
  });
});

describe("moveCursor", () => {
  it("without extend, replaces the selection with the row moved to", () => {
    const s = moveCursor(click("a"), order, 1, false);
    expect([...s.ids]).toEqual(["b"]);
    expect(s.anchor).toBe("b");
  });

  it("clamps at the ends of the order", () => {
    const s = moveCursor(click("a"), order, -1, false);
    expect(s.cursor).toBe("a");
  });

  /** The bug this module exists to prevent: repeated shift+arrow must keep
   * growing the range from one fixed anchor, not restart from the cursor
   * each time. */
  it("with extend, grows the range from a fixed anchor across repeated presses", () => {
    let s = click("b");
    s = moveCursor(s, order, 1, true); // b..c
    s = moveCursor(s, order, 1, true); // b..d
    s = moveCursor(s, order, 1, true); // b..e
    expect(s.anchor).toBe("b");
    expect([...s.ids].sort()).toEqual(["b", "c", "d", "e"]);
  });

  it("with extend, shrinks the range back toward the anchor on reversal", () => {
    let s = click("b");
    s = moveCursor(s, order, 1, true); // b..c
    s = moveCursor(s, order, 1, true); // b..d
    s = moveCursor(s, order, -1, true); // back to b..c
    expect([...s.ids].sort()).toEqual(["b", "c"]);
  });
});

describe("marquee", () => {
  it("replaces the selection when not additive", () => {
    const s = marquee(click("a"), ["c", "d"], false);
    expect([...s.ids].sort()).toEqual(["c", "d"]);
  });

  it("unions with the pre-drag selection when additive", () => {
    const s = marquee(click("a"), ["c", "d"], true);
    expect([...s.ids].sort()).toEqual(["a", "c", "d"]);
  });
});

describe("clear", () => {
  it("returns the empty selection", () => {
    expect(clear()).toEqual(EMPTY_SELECTION);
  });
});

describe("typeAhead", () => {
  const names = new Map([
    ["a", "Apple"],
    ["b", "Banana"],
    ["c", "Bergamo"],
    ["d", "Cherry"],
  ]);

  it("jumps to the first row starting with the typed letter", () => {
    expect(typeAhead(["a", "b", "c", "d"], names, "b", null)).toBe("b");
  });

  it("cycles to the next match on repeated presses of the same letter", () => {
    expect(typeAhead(["a", "b", "c", "d"], names, "b", "b")).toBe("c");
  });

  it("wraps past the end of the list", () => {
    expect(typeAhead(["a", "b", "c", "d"], names, "b", "c")).toBe("b");
  });

  it("returns null when nothing matches", () => {
    expect(typeAhead(["a", "b", "c", "d"], names, "z", null)).toBeNull();
  });
});
