import { describe, expect, it } from "vitest";

import { close, isOpen, open, spineOf, toggle } from "./expansion";

describe("spineOf", () => {
  it("reads the chain out of a placement key", () => {
    expect(spineOf("root>trips>bergamo")).toEqual(["root", "trips", "bergamo"]);
    expect(spineOf("root")).toEqual(["root"]);
    expect(spineOf("")).toEqual([]);
  });
});

describe("toggle", () => {
  it("opens a folder along with everything above it", () => {
    expect(toggle([], "root>trips")).toEqual(["root", "trips"]);
  });

  it("closes any branch that is not on the way to what was opened", () => {
    // The whole point: opening Work must not leave Trips hanging open.
    expect(toggle(["root", "trips"], "root>work")).toEqual(["root", "work"]);
  });

  it("closes a folder that was already open", () => {
    expect(toggle(["root", "trips"], "root>trips")).toEqual(["root"]);
  });

  it("closing a folder closes what was inside it", () => {
    expect(toggle(["root", "trips", "bergamo"], "root>trips")).toEqual(["root"]);
  });

  it("keeps the branch when going deeper", () => {
    expect(toggle(["root", "trips"], "root>trips>bergamo")).toEqual([
      "root",
      "trips",
      "bergamo",
    ]);
  });

  it("never leaves two branches open, whatever the order of clicks", () => {
    let open: string[] = [];
    for (const key of ["root>a", "root>a>a1", "root>b", "root>b>b1", "root>c"]) {
      open = toggle(open, key);
    }
    expect(open).toEqual(["root", "c"]);
  });

  it("handles a top-level folder with no parent above it", () => {
    expect(toggle([], "trips")).toEqual(["trips"]);
    expect(toggle(["trips"], "trips")).toEqual([]);
  });
});

describe("open and close", () => {
  it("open is idempotent where toggle would shut it", () => {
    expect(open(["root", "trips"], "root>trips")).toEqual(["root", "trips"]);
    expect(open([], "root>trips")).toEqual(["root", "trips"]);
  });

  it("close removes the folder and anything under it", () => {
    expect(close("root>trips>bergamo")).toEqual([
      "root",
      "trips",
    ]);
    expect(close("root>trips")).toEqual(["root"]);
  });

  it("closing a folder leaves exactly its ancestors open", () => {
    expect(close("root>trips")).toEqual(["root"]);
    expect(close("trips")).toEqual([]);
  });
});

describe("isOpen", () => {
  it("answers for one id", () => {
    expect(isOpen(["root", "trips"], "trips")).toBe(true);
    expect(isOpen(["root", "trips"], "work")).toBe(false);
  });
});
