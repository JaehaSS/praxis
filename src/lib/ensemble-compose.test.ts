import { describe, expect, it } from "vitest";
import type { EnsembleMatrix, HunkRef } from "./ipc";
import {
  composeSelections,
  conflictingSelections,
  defaultComposeSelection,
  exclusiveGroupOf,
  exclusiveViolations,
  hunkKey,
  summarizeComposeSelection,
  toggleComposeSelection,
} from "./ensemble-compose";

const ref = (task_id: number, hunk_id: string): HunkRef => ({ task_id, hunk_id });

function matrixWithGroup(group: HunkRef[]): EnsembleMatrix {
  return { candidate_ids: [1, 2, 3], exclusive_groups: [group] };
}

describe("hunkKey", () => {
  it("직렬화는 task_id:hunk_id 형식이다", () => {
    expect(hunkKey(ref(1, "h1"))).toBe("1:h1");
  });
});

describe("defaultComposeSelection", () => {
  it("winner의 hunk id들을 미리 선택한 상태로 초기화한다", () => {
    const selection = defaultComposeSelection(1, ["h1", "h2"]);
    expect(selection).toEqual(new Set(["1:h1", "1:h2"]));
  });
});

describe("exclusiveGroupOf", () => {
  it("ref가 속한 배타 그룹을 찾는다", () => {
    const m = matrixWithGroup([ref(1, "w1"), ref(2, "c1")]);
    expect(exclusiveGroupOf(m, ref(2, "c1"))).toEqual([ref(1, "w1"), ref(2, "c1")]);
  });

  it("어떤 그룹에도 속하지 않으면 null", () => {
    const m = matrixWithGroup([ref(1, "w1"), ref(2, "c1")]);
    expect(exclusiveGroupOf(m, ref(3, "solo"))).toBeNull();
  });
});

describe("toggleComposeSelection", () => {
  it("겹치지 않는 hunk는 단순 토글된다", () => {
    const m: EnsembleMatrix = { candidate_ids: [1, 2], exclusive_groups: [] };
    const selected = defaultComposeSelection(1, ["w1"]);
    const next = toggleComposeSelection(selected, m, ref(2, "c1"));
    expect(next).toEqual(new Set(["1:w1", "2:c1"]));
  });

  it("배타 그룹 내 다른 후보 hunk를 선택하면 winner의 겹치는 선택이 자동 해제된다", () => {
    const m = matrixWithGroup([ref(1, "w1"), ref(2, "c1")]);
    const selected = defaultComposeSelection(1, ["w1"]);
    const next = toggleComposeSelection(selected, m, ref(2, "c1"));
    expect(next).toEqual(new Set(["2:c1"]));
  });

  it("3후보 교차 그룹에서 하나를 선택하면 나머지 두 후보의 선택이 모두 해제된다", () => {
    const group = [ref(1, "w1"), ref(2, "c1"), ref(3, "c2")];
    const m = matrixWithGroup(group);
    const selected = new Set(["1:w1", "3:c2"]); // 이미 두 후보가 선택된(비정상) 상태를 가정
    const next = toggleComposeSelection(selected, m, ref(2, "c1"));
    expect(next).toEqual(new Set(["2:c1"]));
  });

  it("이미 선택된 hunk를 다시 토글하면 해제만 되고 그룹 멤버는 건드리지 않는다", () => {
    const m = matrixWithGroup([ref(1, "w1"), ref(2, "c1")]);
    const selected = new Set(["2:c1"]);
    const next = toggleComposeSelection(selected, m, ref(2, "c1"));
    expect(next).toEqual(new Set());
  });
});

describe("exclusiveViolations", () => {
  it("정상 선택(그룹당 최대 1개)에는 위반이 없다", () => {
    const m = matrixWithGroup([ref(1, "w1"), ref(2, "c1")]);
    expect(exclusiveViolations(new Set(["2:c1"]), m)).toEqual([]);
  });

  it("같은 그룹에서 2개 이상 선택되면 위반 그룹을 반환한다(겹침 동시 선택 차단 AC)", () => {
    const group = [ref(1, "w1"), ref(2, "c1")];
    const m = matrixWithGroup(group);
    const violations = exclusiveViolations(new Set(["1:w1", "2:c1"]), m);
    expect(violations).toEqual([group]);
  });
});

describe("composeSelections", () => {
  it("winner 자신의 hunk는 제외하고 타 후보 선택만 반환한다", () => {
    const selected = new Set(["1:w1", "2:c1", "3:c2"]);
    expect(composeSelections(selected, 1)).toEqual([ref(2, "c1"), ref(3, "c2")]);
  });

  it("winner만 선택된 경우(조합 없음) 빈 배열을 반환한다", () => {
    expect(composeSelections(new Set(["1:w1"]), 1)).toEqual([]);
  });
});

describe("summarizeComposeSelection", () => {
  it("후보별 선택 hunk 개수를 센다", () => {
    const selected = new Set(["1:w1", "1:w2", "2:c1"]);
    expect(summarizeComposeSelection(selected)).toEqual({ 1: 2, 2: 1 });
  });
});

describe("conflictingSelections", () => {
  it("배타 그룹의 기존 선택과 겹치면 그 멤버를 경고 대상으로 반환한다", () => {
    const m = matrixWithGroup([ref(1, "w1"), ref(2, "c1")]);
    const selected = defaultComposeSelection(1, ["w1"]);
    expect(conflictingSelections(selected, m, ref(2, "c1"))).toEqual([ref(1, "w1")]);
  });

  it("이미 선택된 hunk를 해제하는 토글은 경고가 없다", () => {
    const m = matrixWithGroup([ref(1, "w1"), ref(2, "c1")]);
    const selected = new Set(["2:c1"]);
    expect(conflictingSelections(selected, m, ref(2, "c1"))).toEqual([]);
  });

  it("배타 그룹에 속하지 않으면 경고가 없다", () => {
    const m: EnsembleMatrix = { candidate_ids: [1, 2], exclusive_groups: [] };
    expect(conflictingSelections(new Set(), m, ref(2, "c1"))).toEqual([]);
  });

  it("겹치는 그룹이라도 기존에 선택된 멤버가 없으면 경고가 없다", () => {
    const m = matrixWithGroup([ref(1, "w1"), ref(2, "c1")]);
    expect(conflictingSelections(new Set(), m, ref(2, "c1"))).toEqual([]);
  });
});
