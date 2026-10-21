import { describe, expect, it } from "vitest";
import { droppedPaths, shellEscape } from "./shell";

describe("shellEscape", () => {
  it("leaves plain paths alone", () => {
    expect(shellEscape("/Users/me/shot.png")).toBe("/Users/me/shot.png");
  });

  it("escapes spaces and shell characters", () => {
    expect(shellEscape("/tmp/Screen Shot (1).png")).toBe("/tmp/Screen\\ Shot\\ \\(1\\).png");
    expect(shellEscape("/tmp/a'b$c&d.png")).toBe("/tmp/a\\'b\\$c\\&d.png");
  });
});

describe("droppedPaths", () => {
  it("joins several files and ends with a space so typing can continue", () => {
    expect(droppedPaths(["/a b.png", "/c.png"])).toBe("/a\\ b.png /c.png ");
  });
});
