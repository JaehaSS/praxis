import { describe, expect, it } from "vitest";
import { previewBoundsChanged, previewBoundsFromRect } from "./layout";

describe("Design Mode preview layout", () => {
  it("rejects collapsed or non-finite preview containers", () => {
    expect(previewBoundsFromRect({ x: 0, y: 0, width: 0, height: 300 })).toBeNull();
    expect(
      previewBoundsFromRect({ x: Number.NaN, y: 0, width: 400, height: 300 }),
    ).toBeNull();
  });

  it("normalizes subpixel bounds before native synchronization", () => {
    expect(
      previewBoundsFromRect({ x: 12.24, y: 40.26, width: 800.74, height: 600.76 }),
    ).toEqual({ x: 12, y: 40.5, width: 800.5, height: 601 });
  });

  it("skips duplicate native bounds updates", () => {
    const current = { x: 12, y: 40.5, width: 800.5, height: 601 };

    expect(previewBoundsChanged(current, { ...current })).toBe(false);
    expect(previewBoundsChanged(current, { ...current, width: 801 })).toBe(true);
    expect(previewBoundsChanged(null, current)).toBe(true);
  });
});
