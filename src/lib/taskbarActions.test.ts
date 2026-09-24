import { describe, expect, it } from "vitest";

import { barActions, barStatus, type BarContext } from "./taskbarActions";
import { dragIds, parseIds } from "./drag";

const base: BarContext = { pane: "library", selection: [], tray: [], scope: null, inspecting: null };
const ids = (ctx: Partial<BarContext>) => barActions({ ...base, ...ctx }).map((a) => a.id);
const find = (ctx: Partial<BarContext>, id: string) =>
  barActions({ ...base, ...ctx }).find((a) => a.id === id);

describe("the taskbar's actions", () => {
  it("with nothing selected offers making and a disabled tag that says why", () => {
    expect(ids({})).toEqual(["new", "tag"]);
    const tag = find({}, "tag")!;
    expect(tag.enabled).toBe(false);
    expect(tag.title).toMatch(/select/i);
  });

  it("grows with a selection in a list", () => {
    expect(ids({ selection: ["a", "b"] })).toEqual(["new", "tag", "toTray", "inspect", "delete"]);
    expect(find({ selection: ["a", "b"] }, "tag")!.label).toBe("# Tag 2");
    expect(find({ selection: ["a", "b"] }, "delete")!.label).toBe("Delete 2");
  });

  it("does not offer to inspect from the Inspector, or delete from it", () => {
    const got = ids({ pane: "inspector", selection: ["a"] });
    expect(got).not.toContain("inspect");
    expect(got).not.toContain("delete");
    expect(got).toContain("tag");
  });

  it("only offers the tray for what is not already in it", () => {
    expect(ids({ selection: ["a"], tray: ["a"] })).not.toContain("toTray");
    expect(find({ selection: ["a", "b", "c"], tray: ["a"] }, "toTray")!.label).toBe("⊕ 2 to tray");
  });

  it("offers to put the tray in the collector the Viewer is showing", () => {
    const scope = { id: "f", name: "Picks", kind: "folder" as const };
    const a = find({ pane: "viewer", tray: ["x", "y"], scope }, "gather")!;
    expect(a.label).toBe("Add 2 to Picks");
    expect(a.primary).toBe(true);
    expect(find({ pane: "viewer", scope }, "new")!.label).toBe("＋ New in Picks");
  });

  it("never offers a button about nothing", () => {
    // No scope — the library root, or a mirrored folder — no gather.
    expect(ids({ pane: "viewer", tray: ["x"] })).not.toContain("gather");
    // The same tray in the Library: the Library is not a place to put things.
    expect(ids({ pane: "library", tray: ["x"] })).not.toContain("gather");
    // A tray holding only the folder itself has nothing to put in it.
    const scope = { id: "f", name: "Picks", kind: "folder" as const };
    expect(ids({ pane: "viewer", tray: ["f"], scope })).not.toContain("gather");
  });

  it("offers to link the tray from the Inspector, not counting the item itself", () => {
    const inspecting = { id: "a", name: "alpha" };
    const a = find({ pane: "inspector", tray: ["a", "b", "c"], inspecting }, "link")!;
    expect(a.label).toBe("Link 2 to alpha…");
    expect(ids({ pane: "inspector", tray: ["a"], inspecting })).not.toContain("link");
  });

  it("has at most one primary action", () => {
    const scope = { id: "f", name: "Picks", kind: "folder" as const };
    for (const pane of ["library", "scattered", "viewer", "inspector"] as const) {
      const primaries = barActions({
        pane,
        selection: ["a"],
        tray: ["x"],
        scope,
        inspecting: { id: "a", name: "alpha" },
      }).filter((a) => a.primary);
      expect(primaries.length).toBeLessThanOrEqual(1);
    }
  });

  it("says how much is selected", () => {
    expect(barStatus(base)).toBe("Nothing selected");
    expect(barStatus({ ...base, selection: ["a"] })).toBe("1 item selected");
    expect(barStatus({ ...base, selection: ["a", "b"] })).toBe("2 items selected");
  });
});

describe("dragging items", () => {
  it("carries the whole selection when the row is part of it", () => {
    expect(dragIds("b", ["a", "b", "c"])).toEqual(["a", "b", "c"]);
    expect(dragIds("z", ["a", "b"])).toEqual(["z"]);
  });

  it("reads back only what an item drag writes", () => {
    expect(parseIds(JSON.stringify(["a", "b"]))).toEqual(["a", "b"]);
    for (const junk of [null, "", "not json", "{}", "[1,2]", '["a", 3]', '[""]']) {
      expect(parseIds(junk)).toEqual([]);
    }
  });
});
