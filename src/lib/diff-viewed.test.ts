// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clearViewed,
  fingerprint,
  isViewed,
  loadViewed,
  markViewed,
  saveViewed,
  viewedCount,
} from "./diff-viewed";

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("viewed state persistence", () => {
  it("round-trips through localStorage per task", () => {
    saveViewed(7, markViewed({}, "a.ts", "PATCH"));
    expect(isViewed(loadViewed(7), "a.ts", "PATCH")).toBe(true);
    expect(loadViewed(8)).toEqual({});
  });

  it("falls back to empty state on malformed JSON", () => {
    localStorage.setItem("praxis:diff-viewed:7", "{not json");
    expect(loadViewed(7)).toEqual({});
  });

  it("rejects a stored value that is not an object", () => {
    localStorage.setItem("praxis:diff-viewed:7", "5");
    expect(loadViewed(7)).toEqual({});
  });

  it("treats a stored null as empty rather than crashing later", () => {
    localStorage.setItem("praxis:diff-viewed:7", "null");
    expect(loadViewed(7)).toEqual({});
  });

  it("survives storage being denied on read", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("private mode");
    });
    expect(loadViewed(7)).toEqual({});
  });

  it("survives storage being denied on write", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    expect(() => saveViewed(7, { "a.ts": "x" })).not.toThrow();
  });
});

describe("diff viewed state", () => {
  it("keeps a file viewed while its patch is unchanged", () => {
    const state = markViewed({}, "a.ts", "PATCH");
    expect(isViewed(state, "a.ts", "PATCH")).toBe(true);
  });

  it("drops the viewed mark when the patch changes", () => {
    const state = markViewed({}, "a.ts", "PATCH");
    expect(isViewed(state, "a.ts", "PATCH-v2")).toBe(false);
  });

  it("treats an unseen file as not viewed", () => {
    expect(isViewed({}, "a.ts", "PATCH")).toBe(false);
  });

  it("clears an explicit unmark", () => {
    const state = clearViewed(markViewed({}, "a.ts", "PATCH"), "a.ts");
    expect(isViewed(state, "a.ts", "PATCH")).toBe(false);
  });

  it("counts only files whose current patch is still viewed", () => {
    const state = markViewed(markViewed({}, "a.ts", "A"), "b.ts", "B");
    expect(
      viewedCount(state, [
        { path: "a.ts", patch: "A" },
        { path: "b.ts", patch: "B-changed" },
      ]),
    ).toBe(1);
  });

  it("distinguishes patches of the same length", () => {
    expect(fingerprint("abcd")).not.toBe(fingerprint("abce"));
  });

  it("does not mutate the state it is given", () => {
    const state = markViewed({}, "a.ts", "A");
    markViewed(state, "b.ts", "B");
    clearViewed(state, "a.ts");
    expect(Object.keys(state)).toEqual(["a.ts"]);
  });
});
