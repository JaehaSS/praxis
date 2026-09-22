import { describe, expect, it } from "vitest";
import {
  defaultPartialSelection,
  summarizePartialSelection,
  togglePartialSelection,
} from "./partial";
import type { DiffHunk } from "./ipc";

function hunk(id: string, protected_ = false, committed = false): DiffHunk {
  return {
    id,
    path: "a.ts",
    old_range: [1, 1],
    new_range: [1, 1],
    lines: [],
    protected: protected_,
    committed,
    risk: "low",
  };
}

describe("defaultPartialSelection", () => {
  it("선택 후보에서 protected hunk를 제외하고 나머지를 모두 유지 선택한다", () => {
    const hunks = [hunk("h1"), hunk("h2", true), hunk("h3")];

    expect(defaultPartialSelection(hunks)).toEqual(new Set(["h1", "h3"]));
  });
});

describe("togglePartialSelection", () => {
  it("일반 hunk는 선택/해제를 토글한다", () => {
    const selected = new Set(["h1"]);

    expect(togglePartialSelection(selected, hunk("h1"))).toEqual(new Set());
    expect(togglePartialSelection(selected, hunk("h2"))).toEqual(new Set(["h1", "h2"]));
  });

  it("protected hunk는 토글해도 선택 상태가 변하지 않는다(이중 방어)", () => {
    const selected = new Set(["h1"]);

    expect(togglePartialSelection(selected, hunk("h2", true))).toBe(selected);
  });
});

describe("summarizePartialSelection", () => {
  it("선택된 hunk는 kept, 그 외는 discarded로 분류한다", () => {
    const hunks = [hunk("h1"), hunk("h2", true), hunk("h3")];
    const selected = new Set(["h1"]);

    expect(summarizePartialSelection(hunks, selected)).toEqual({
      keptIds: ["h1"],
      discardedIds: ["h2", "h3"],
    });
  });
});

describe("committed hunk", () => {
  const pending = hunk("pending");
  const shipped = hunk("shipped", false, true);
  const hunks = [pending, shipped];

  it("is not selected by default", () => {
    expect(defaultPartialSelection(hunks)).toEqual(new Set(["pending"]));
  });

  it("cannot be toggled on", () => {
    const before = new Set<string>();
    expect(togglePartialSelection(before, shipped)).toBe(before);
  });

  it("counts as neither kept nor discarded", () => {
    // 양쪽에서 빠져야 한다. discarded에 들어가면 확인 문구가 지우지도 않을 것을 센다.
    const summary = summarizePartialSelection(hunks, new Set<string>());
    expect(summary.keptIds).toEqual([]);
    expect(summary.discardedIds).toEqual(["pending"]);
  });
});
