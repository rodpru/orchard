import { describe, expect, it } from "vitest";
import { withItem, withoutItem, withoutKey } from "./collections";

describe("collections", () => {
  it("returns the same object when nothing changes, so React skips the render", () => {
    const set = new Set(["a"]);
    expect(withItem(set, "a")).toBe(set);
    expect(withoutItem(set, "b")).toBe(set);
    const record = { a: 1 };
    expect(withoutKey(record, "b")).toBe(record);
  });

  it("copies on change", () => {
    const set = new Set(["a"]);
    expect([...withItem(set, "b")]).toEqual(["a", "b"]);
    expect([...withoutItem(set, "a")]).toEqual([]);
    expect(withoutKey({ a: 1, b: 2 }, "a")).toEqual({ b: 2 });
    expect([...set]).toEqual(["a"]);
  });
});
