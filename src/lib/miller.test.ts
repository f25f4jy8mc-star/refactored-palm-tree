import { describe, expect, it } from "vitest";

import { highlighted, select, step, type Cascade } from "./miller";

// Column 0: A (a folder), b, c. Column 1 is inside A: x, y.
const col0 = ["A", "b", "c"];
const col1 = ["x", "y"];
const start: Cascade = { path: [], cursor: "A" };

describe("the Miller cascade", () => {
  it("coming back out of a folder and stepping down goes to the next row, and the one after", () => {
    // Open A and move to y inside it.
    let c = select(start, 0, "A", true);
    c = { ...c, cursor: "x" }; // the view highlights the first row on arrival
    c = select(c, 1, step(col1, highlighted(c, 1), 1)!, false);
    expect(highlighted(c, 1)).toBe("y");

    // ← back to column 0: A is still the highlight there.
    expect(highlighted(c, 0)).toBe("A");

    // ↓ lands on b, and b is what is highlighted afterwards — not A, not the top.
    c = select(c, 0, step(col0, highlighted(c, 0), 1)!, false);
    expect(highlighted(c, 0)).toBe("b");
    expect(c.path).toEqual([]);

    // ↓ again is c, the next in sequence. This is where it used to jump.
    c = select(c, 0, step(col0, highlighted(c, 0), 1)!, false);
    expect(highlighted(c, 0)).toBe("c");
  });

  it("stepping up out of a folder works the same way", () => {
    let c = select({ path: [], cursor: "c" }, 0, "A", true);
    c = { ...c, cursor: "x" };
    c = select(c, 0, "b", false);
    c = select(c, 0, step(col0, highlighted(c, 0), -1)!, true);
    expect(highlighted(c, 0)).toBe("A");
    expect(c.path).toEqual(["A"]);
  });

  it("selecting the folder already open keeps its column as it was", () => {
    const c: Cascade = { path: ["A"], cursor: "y" };
    expect(select(c, 0, "A", true)).toBe(c);
  });

  it("selecting a different folder opens it fresh", () => {
    const c = select({ path: ["A"], cursor: "y" }, 0, "B", true);
    expect(c).toEqual({ path: ["B"], cursor: null });
  });

  it("stops at the ends rather than wrapping", () => {
    expect(step(col0, "c", 1)).toBe("c");
    expect(step(col0, "A", -1)).toBe("A");
    expect(step(col0, null, 1)).toBe("A");
    expect(step([], "A", 1)).toBe(null);
  });
});
