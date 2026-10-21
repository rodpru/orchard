import { describe, expect, it } from "vitest";
import { shortcutKey, shortcutLabel } from "./platform";

const key = (overrides: Partial<Parameters<typeof shortcutKey>[0]>) => ({
  key: "",
  code: "",
  metaKey: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  ...overrides,
});

describe("shortcutKey", () => {
  it("uses ⌘ on macOS", () => {
    expect(shortcutKey(key({ key: "n", code: "KeyN", metaKey: true }), true)).toBe("n");
    expect(shortcutKey(key({ key: "3", code: "Digit3", metaKey: true }), true)).toBe("3");
    expect(shortcutKey(key({ key: "n", code: "KeyN", ctrlKey: true, shiftKey: true }), true)).toBeNull();
  });

  it("uses Ctrl+Shift on Linux and reads digits from the physical key", () => {
    expect(shortcutKey(key({ key: "N", code: "KeyN", ctrlKey: true, shiftKey: true }), false)).toBe("n");
    expect(shortcutKey(key({ key: "#", code: "Digit3", ctrlKey: true, shiftKey: true }), false)).toBe("3");
  });

  it("leaves plain Ctrl and Alt combinations to the terminal on Linux", () => {
    expect(shortcutKey(key({ key: "n", code: "KeyN", ctrlKey: true }), false)).toBeNull();
    expect(shortcutKey(key({ key: "N", code: "KeyN", ctrlKey: true, shiftKey: true, altKey: true }), false)).toBeNull();
    expect(shortcutKey(key({ key: "n", code: "KeyN", metaKey: true }), false)).toBeNull();
  });
});

describe("shortcutLabel", () => {
  it("names the modifier of each platform", () => {
    expect(shortcutLabel("n", true)).toBe("⌘N");
    expect(shortcutLabel("n", false)).toBe("Ctrl+Shift+N");
  });
});
