import { describe, expect, it } from "vitest";
import {
  activateInGroup,
  canDropOntoGroup,
  closeGroup,
  closeKeyInGroup,
  createLayout,
  dropOntoGroup,
  evenSizes,
  focusedGroup,
  focusGroup,
  isOpenInAnyGroup,
  MAX_GROUPS,
  MIN_GROUP_PX,
  resizeBoundary,
  splitFocused,
  syncLayout,
  type SplitLayout,
  type TabMarks,
} from "./editor-split";
import { fileTabKey, type TabKey } from "../../lib/tab-key";

/** 이 모듈은 키만 다룬다. 테스트는 경로처럼 읽는 편이 나으므로 파일 탭 키로 브랜드만 씌운다. */
const k = (path: string): TabKey => fileTabKey(path);

/** 비중의 총합. 칸 수보다 작으면 flex-grow가 컨테이너를 다 못 채운다 — 그 자체가 결함이다. */
const sizeSum = (layout: SplitLayout): number =>
  layout.groups.reduce((total, g) => total + g.size, 0);

/** 세 칸(상한) 배치 — g0=[a,b] · g1=[b] · g2=[b](포커스). */
const threeGroups = (): SplitLayout =>
  splitFocused(splitFocused(createLayout([k("a.ts"), k("b.ts")], k("b.ts")), "row"), "row");

/** 두 그룹 배치 — 왼쪽에 a·b(활성 b), 오른쪽에 b(분할로 복제됨). */
const twoGroups = (): SplitLayout => splitFocused(createLayout([k("a.ts"), k("b.ts")], k("b.ts")), "row");

/** 대부분의 테스트는 배치만 본다 — 쫓겨난 탭까지 볼 때만 `syncLayout`을 그대로 부른다. */
const sync = (
  layout: SplitLayout,
  keys: TabKey[],
  active: TabKey | null,
  marks?: TabMarks,
): SplitLayout => syncLayout(layout, keys, active, marks).layout;

describe("createLayout", () => {
  it("활성 경로가 목록에 없으면 마지막 파일을 띄운다", () => {
    const layout = createLayout([k("a.ts"), k("b.ts")], k("gone.ts"));
    expect(focusedGroup(layout).activeKey).toBe("b.ts");
  });

  it("파일이 없으면 활성도 없다", () => {
    expect(focusedGroup(createLayout()).activeKey).toBeNull();
  });
});

describe("splitFocused", () => {
  it("새 그룹이 보던 파일을 들고 포커스를 가져간다", () => {
    const layout = twoGroups();
    expect(layout.groups).toHaveLength(2);
    expect(layout.groups[1].keys).toEqual(["b.ts"]);
    expect(layout.focusedId).toBe(layout.groups[1].id);
    // 자기 몫을 반으로 나누고(0.5씩) 합을 칸 수로 되돌린다 — 비율만 남으므로 1:1.
    expect(layout.groups.map((g) => g.size)).toEqual([1, 1]);
  });

  it("id를 재사용하지 않는다", () => {
    const layout = splitFocused(twoGroups(), "row");
    expect(new Set(layout.groups.map((g) => g.id)).size).toBe(layout.groups.length);
  });

  it("상한에 닿으면 그룹을 늘리지 않고 축만 돌린다", () => {
    let layout = createLayout([k("a.ts")], k("a.ts"));
    for (let i = 0; i < MAX_GROUPS + 2; i += 1) layout = splitFocused(layout, "row");
    expect(layout.groups).toHaveLength(MAX_GROUPS);
    expect(splitFocused(layout, "column").axis).toBe("column");
    expect(splitFocused(layout, "column").groups).toHaveLength(MAX_GROUPS);
  });

  it("띄운 파일이 없으면 쪼개지 않는다", () => {
    const layout = createLayout();
    expect(splitFocused(layout, "row").groups).toHaveLength(1);
  });
});

describe("closeKeyInGroup", () => {
  it("그 그룹에서만 닫는다 — 다른 그룹의 같은 파일은 남는다", () => {
    const layout = closeKeyInGroup(twoGroups(), "g0", k("b.ts"));
    expect(layout.groups[0].keys).toEqual(["a.ts"]);
    expect(isOpenInAnyGroup(layout, k("b.ts"))).toBe(true);
  });

  it("마지막 탭을 닫으면 그룹이 사라지고 포커스가 이웃으로 간다", () => {
    const split = twoGroups();
    const layout = closeKeyInGroup(split, split.groups[1].id, k("b.ts"));
    expect(layout.groups).toHaveLength(1);
    expect(layout.focusedId).toBe("g0");
    expect(isOpenInAnyGroup(layout, k("b.ts"))).toBe(true);
  });

  it("마지막 그룹의 마지막 탭은 그룹을 남긴 채 빈다", () => {
    const layout = closeKeyInGroup(createLayout([k("a.ts")], k("a.ts")), "g0", k("a.ts"));
    expect(layout.groups).toHaveLength(1);
    expect(layout.groups[0].activeKey).toBeNull();
  });

  it("활성 탭을 닫으면 오른쪽 이웃이 승계한다", () => {
    const layout = closeKeyInGroup(createLayout([k("a.ts"), k("b.ts"), k("c.ts")], k("b.ts")), "g0", k("b.ts"));
    expect(layout.groups[0].activeKey).toBe("c.ts");
  });

  it("맨 끝 탭을 닫으면 왼쪽으로 승계한다", () => {
    const layout = closeKeyInGroup(createLayout([k("a.ts"), k("b.ts")], k("b.ts")), "g0", k("b.ts"));
    expect(layout.groups[0].activeKey).toBe("a.ts");
  });
});

describe("closeGroup", () => {
  it("들고 있던 탭을 이웃으로 넘기고 사라진다 — 파일을 닫지 않는다", () => {
    const split = activateInGroup(twoGroups(), "g0", k("a.ts"));
    const withExtra = activateInGroup(split, split.groups[1].id, k("c.ts"));
    const layout = closeGroup(withExtra, withExtra.groups[1].id);
    expect(layout.groups).toHaveLength(1);
    expect(layout.groups[0].keys).toEqual(["a.ts", "b.ts", "c.ts"]);
    expect(layout.groups[0].activeKey).toBe("c.ts");
  });

  it("그룹이 하나뿐이면 아무것도 하지 않는다", () => {
    const layout = createLayout([k("a.ts")], k("a.ts"));
    expect(closeGroup(layout, "g0")).toBe(layout);
  });
});

describe("resizeBoundary", () => {
  it("픽셀 델타를 비중으로 환산한다", () => {
    const layout = resizeBoundary(twoGroups(), 0, 200, 1000);
    // 1000px에 1+1 → 1비중당 500px. 200px = 0.4비중.
    expect(layout.groups[0].size).toBeCloseTo(1.4);
    expect(layout.groups[1].size).toBeCloseTo(0.6);
  });

  it("끌어도 총합은 변하지 않는다 — 남는 여백이 생기면 안 된다", () => {
    const before = twoGroups();
    const after = resizeBoundary(before, 0, 200, 1000);
    const sum = (l: SplitLayout) => l.groups.reduce((t, g) => t + g.size, 0);
    expect(sum(after)).toBeCloseTo(sum(before));
  });

  it("양쪽 모두 최소 폭을 지킨다", () => {
    const layout = resizeBoundary(twoGroups(), 0, 9999, 1000);
    const unit = 1000 / layout.groups.reduce((t, g) => t + g.size, 0);
    expect(layout.groups[1].size * unit).toBeCloseTo(MIN_GROUP_PX);
  });

  it("둘이 합쳐도 최소 폭 둘을 못 채우면 끌리지 않는다", () => {
    const layout = twoGroups();
    expect(resizeBoundary(layout, 0, 40, MIN_GROUP_PX)).toBe(layout);
  });

  it("없는 경계는 무시한다", () => {
    const layout = twoGroups();
    expect(resizeBoundary(layout, 5, 40, 1000)).toBe(layout);
  });
});

describe("syncLayout", () => {
  it("바뀐 것이 없으면 같은 참조를 돌려준다", () => {
    const layout = twoGroups();
    expect(sync(layout, [k("a.ts"), k("b.ts")], k("b.ts"))).toBe(layout);
  });

  it("전역에서 닫힌 파일을 모든 그룹에서 뺀다", () => {
    const layout = sync(twoGroups(), [k("a.ts")], k("a.ts"));
    // b.ts만 들고 있던 오른쪽 그룹은 비어 사라진다.
    expect(layout.groups).toHaveLength(1);
    expect(layout.groups[0].keys).toEqual(["a.ts"]);
    expect(layout.focusedId).toBe("g0");
  });

  it("새로 열린 파일은 포커스된 그룹으로 들어간다", () => {
    const split = twoGroups();
    const layout = sync(split, [k("a.ts"), k("b.ts"), k("c.ts")], k("c.ts"));
    expect(layout.groups[0].keys).toEqual(["a.ts", "b.ts"]);
    expect(layout.groups[1].keys).toEqual(["b.ts", "c.ts"]);
    expect(layout.groups[1].activeKey).toBe("c.ts");
  });

  it("이미 다른 그룹에 열린 파일로 활성이 바뀌면 포커스가 그리로 넘어간다", () => {
    const split = twoGroups();
    const layout = sync(split, [k("a.ts"), k("b.ts")], k("a.ts"));
    expect(layout.focusedId).toBe("g0");
    expect(layout.groups[0].activeKey).toBe("a.ts");
  });

  it("포커스된 그룹이 들고 있으면 포커스를 옮기지 않는다", () => {
    const split = twoGroups();
    const withBoth = activateInGroup(split, split.groups[1].id, k("a.ts"));
    const layout = sync(withBoth, [k("a.ts"), k("b.ts")], k("b.ts"));
    expect(layout.focusedId).toBe(split.groups[1].id);
    expect(focusedGroup(layout).activeKey).toBe("b.ts");
  });

  it("작업이 바뀌어 파일이 모두 사라지면 한 그룹으로 접힌다", () => {
    const layout = sync(twoGroups(), [], null);
    expect(layout.groups).toHaveLength(1);
    expect(layout.groups[0].activeKey).toBeNull();
  });
});

describe("syncLayout — 프리뷰 자리", () => {
  /** 포커스된 칸(g1)이 프리뷰 p.ts를 들고 있는 두 칸 배치 — g0=[a] · g1=[p](포커스). */
  const withPreview = (): SplitLayout => {
    const split = splitFocused(createLayout([k("a.ts")], k("a.ts")), "row");
    return closeKeyInGroup(activateInGroup(sync(split, [k("a.ts"), k("p.ts")], k("p.ts")), "g1", k("p.ts")), "g1", k("a.ts"));
  };
  const marks = (preview: string[], dirty: string[] = []): TabMarks => ({
    preview: new Set(preview.map(k)),
    dirty: new Set(dirty.map(k)),
  });

  it("프리뷰 탭이 있던 자리를 새 프리뷰가 그대로 물려받는다", () => {
    const layout = createLayout([k("a.ts"), k("p.ts"), k("c.ts")], k("p.ts"));
    const { layout: next, evicted } = syncLayout(layout, [k("a.ts"), k("p.ts"), k("c.ts"), k("new.ts")], k("new.ts"), marks(["p.ts", "new.ts"]));
    expect(next.groups[0].keys).toEqual(["a.ts", "new.ts", "c.ts"]);
    expect(next.groups[0].activeKey).toBe("new.ts");
    expect(evicted).toEqual(["p.ts"]);
  });

  it("옆 칸이 아직 들고 있으면 전역에서 닫지 않는다", () => {
    const base = withPreview();
    const shared = sync(base, [k("a.ts"), k("p.ts")], k("p.ts"));
    const withCopy = activateInGroup(dropOntoGroup(shared, k("p.ts"), "g0", "center", "g0"), "g1", k("p.ts"));
    const { layout: next, evicted } = syncLayout(withCopy, [k("a.ts"), k("p.ts"), k("new.ts")], k("new.ts"), marks(["p.ts", "new.ts"]));
    expect(next.groups[1].keys).toEqual(["new.ts"]);
    expect(next.groups[0].keys).toContain("p.ts");
    expect(evicted).toEqual([]);
  });

  it("편집 중인 프리뷰는 밀어내지 않는다 — 탭이 하나 는다", () => {
    const { layout: next, evicted } = syncLayout(withPreview(), [k("a.ts"), k("p.ts"), k("new.ts")], k("new.ts"), marks(["p.ts", "new.ts"], ["p.ts"]));
    expect(next.groups[1].keys).toEqual(["p.ts", "new.ts"]);
    expect(evicted).toEqual([]);
  });

  it("다른 칸의 프리뷰는 건드리지 않는다", () => {
    const base = withPreview();
    const { layout: next } = syncLayout(base, [k("a.ts"), k("p.ts"), k("new.ts")], k("new.ts"), marks(["a.ts", "p.ts", "new.ts"]));
    expect(next.groups[0].keys).toEqual(["a.ts"]);
  });

  it("고정 탭만 있는 칸에서는 끝에 붙인다", () => {
    const { layout: next, evicted } = syncLayout(createLayout([k("a.ts")], k("a.ts")), [k("a.ts"), k("new.ts")], k("new.ts"), marks(["new.ts"]));
    expect(next.groups[0].keys).toEqual(["a.ts", "new.ts"]);
    expect(evicted).toEqual([]);
  });

  it("새 탭이 프리뷰가 아니면 자리를 뺏지 않는다", () => {
    const { layout: next } = syncLayout(withPreview(), [k("a.ts"), k("p.ts"), k("new.ts")], k("new.ts"), marks(["p.ts"]));
    expect(next.groups[1].keys).toEqual(["p.ts", "new.ts"]);
  });

  it("한 렌더에 온 프리뷰 여럿도 차례로 교체해 칸에는 마지막 하나만 남긴다", () => {
    const { layout: next, evicted } = syncLayout(
      withPreview(),
      [k("a.ts"), k("p.ts"), k("x.ts"), k("y.ts")],
      k("y.ts"),
      marks(["p.ts", "x.ts", "y.ts"]),
    );
    expect(next.groups[1].keys).toEqual(["y.ts"]);
    expect(evicted).toEqual(["p.ts", "x.ts"]);
  });
});

describe("focusGroup", () => {
  it("없는 그룹으로는 옮기지 않는다", () => {
    const layout = twoGroups();
    expect(focusGroup(layout, "없음")).toBe(layout);
  });
});

describe("비중 정규화", () => {
  it("칸이 사라져도 총합이 칸 수를 따라온다", () => {
    expect(sizeSum(twoGroups())).toBeCloseTo(2);
    expect(sizeSum(threeGroups())).toBeCloseTo(3);
    expect(sizeSum(closeGroup(threeGroups(), "g2"))).toBeCloseTo(2);
  });

  it("칸을 다 비웠다 다시 열어도 폭을 온전히 되찾는다", () => {
    // 결함 그대로의 재현: 세 칸으로 나눈 뒤 파일을 전부 닫고 새 파일 하나를 연다.
    // 정규화가 없으면 남은 칸이 0.25를 물려받아 화면의 1/4만 차지한다.
    const emptied = sync(threeGroups(), [], null);
    expect(emptied.groups).toHaveLength(1);
    expect(emptied.groups[0].size).toBe(1);

    const reopened = sync(emptied, [k("c.ts")], k("c.ts"));
    expect(reopened.groups[0].size).toBe(1);
  });

  it("칸이 접혀도 남은 칸끼리의 비율은 그대로다 — 총합만 되돌린다", () => {
    const layout = threeGroups();
    const ratio = layout.groups[0].size / layout.groups[1].size;
    const dropped = closeGroup(layout, "g2");
    expect(dropped.groups[0].size / dropped.groups[1].size).toBeCloseTo(ratio);
    expect(sizeSum(dropped)).toBeCloseTo(2);
  });

  it("evenSizes는 모든 칸을 같은 폭으로 되돌린다", () => {
    const even = evenSizes(resizeBoundary(twoGroups(), 0, 250, 1000));
    expect(even.groups.map((g) => g.size)).toEqual([1, 1]);
    expect(evenSizes(even)).toBe(even);
  });
});

describe("dropOntoGroup", () => {
  it("가장자리에 떨구면 새 칸이 생기고 원본에서는 사라진다 — 복제가 아니다", () => {
    const layout = dropOntoGroup(createLayout([k("a.ts"), k("b.ts")], k("b.ts")), k("a.ts"), "g0", "right", "g0");
    expect(layout.groups.map((g) => g.keys)).toEqual([["b.ts"], ["a.ts"]]);
    expect(layout.focusedId).toBe(layout.groups[1].id);
    expect(sizeSum(layout)).toBeCloseTo(2);
  });

  it("앞쪽 가장자리는 앞에 꽂는다", () => {
    const layout = dropOntoGroup(createLayout([k("a.ts"), k("b.ts")], k("b.ts")), k("a.ts"), "g0", "left", "g0");
    expect(layout.groups.map((g) => g.keys)).toEqual([["a.ts"], ["b.ts"]]);
  });

  it("위·아래로 떨구면 배치가 세로로 돈다", () => {
    const layout = dropOntoGroup(createLayout([k("a.ts"), k("b.ts")], k("b.ts")), k("a.ts"), "g0", "bottom", "g0");
    expect(layout.axis).toBe("column");
    expect(layout.groups.map((g) => g.keys)).toEqual([["b.ts"], ["a.ts"]]);
  });

  it("가운데로 떨구면 그 칸으로 옮겨 간다", () => {
    const layout = dropOntoGroup(twoGroups(), k("a.ts"), "g1", "center", "g0");
    expect(layout.groups[0].keys).toEqual(["b.ts"]);
    expect(layout.groups[1].keys).toEqual(["b.ts", "a.ts"]);
    expect(focusedGroup(layout).activeKey).toBe("a.ts");
  });

  it("자기 칸의 유일한 탭을 자기 가장자리에 떨구면 아무 일도 없다", () => {
    const layout = createLayout([k("a.ts")], k("a.ts"));
    expect(dropOntoGroup(layout, k("a.ts"), "g0", "right", "g0")).toBe(layout);
    expect(canDropOntoGroup(layout, k("a.ts"), "g0", "right", "g0")).toBe(false);
  });

  it("상한에 닿으면 새 칸을 만들지 않는다", () => {
    const layout = threeGroups();
    expect(layout.groups).toHaveLength(MAX_GROUPS);
    expect(dropOntoGroup(layout, k("a.ts"), "g1", "right", "g0")).toBe(layout);
  });

  it("원본 칸이 비면 상한에 걸리지 않는다 — 하나가 접히고 하나가 생긴다", () => {
    const layout = dropOntoGroup(threeGroups(), k("b.ts"), "g0", "left", "g2");
    expect(layout.groups).toHaveLength(MAX_GROUPS);
    expect(layout.groups.map((g) => g.id)).not.toContain("g2");
    expect(layout.groups[0].keys).toEqual(["b.ts"]);
  });

  it("어느 칸에도 없던 파일은 원본 없이 새 칸으로 들어간다", () => {
    const layout = dropOntoGroup(createLayout([k("a.ts")], k("a.ts")), k("새.ts"), "g0", "right", null);
    expect(layout.groups.map((g) => g.keys)).toEqual([["a.ts"], ["새.ts"]]);
  });

  it("없는 칸을 대상으로 하면 무시한다", () => {
    const layout = twoGroups();
    expect(dropOntoGroup(layout, k("a.ts"), "없음", "right", "g0")).toBe(layout);
  });
});

describe("탭 순서 바꾸기", () => {
  const three = (): SplitLayout => createLayout([k("a.ts"), k("b.ts"), k("c.ts")], k("a.ts"));

  it("자리를 주면 그 자리로 옮긴다", () => {
    const next = activateInGroup(three(), "g0", k("c.ts"), 0);
    expect(next.groups[0].keys).toEqual(["c.ts", "a.ts", "b.ts"]);
    expect(next.groups[0].activeKey).toBe("c.ts");
  });

  it("오른쪽으로 한 칸 — 자기 자리를 뺀 뒤에 센다", () => {
    // 자기를 빼지 않고 세면 a는 제자리에 머문다(자기가 차지한 만큼 목표가 밀린다).
    expect(activateInGroup(three(), "g0", k("a.ts"), 1).groups[0].keys).toEqual([
      "b.ts",
      "a.ts",
      "c.ts",
    ]);
  });

  it("범위를 넘겨도 끝으로 붙을 뿐 사라지지 않는다", () => {
    expect(activateInGroup(three(), "g0", k("a.ts"), 99).groups[0].keys).toEqual([
      "b.ts",
      "c.ts",
      "a.ts",
    ]);
  });

  it("자리를 안 주면 순서는 그대로다 — 예전 호출부의 동작", () => {
    expect(activateInGroup(three(), "g0", k("c.ts")).groups[0].keys).toEqual([
      "a.ts",
      "b.ts",
      "c.ts",
    ]);
  });

  it("탭 바에 떨구면 그 자리에 끼운다 — 같은 칸이어도 순서가 바뀐다", () => {
    const next = dropOntoGroup(three(), k("c.ts"), "g0", "center", "g0", 1);
    expect(next.groups[0].keys).toEqual(["a.ts", "c.ts", "b.ts"]);
  });

  it("다른 칸에서 온 탭도 지정한 자리에 앉는다", () => {
    const split = splitFocused(createLayout([k("a.ts"), k("b.ts")], k("b.ts")), "row");
    // 오른쪽 칸이 b.ts를 들고 태어난다 — 그것을 왼쪽 칸의 맨 앞으로 옮긴다.
    const moved = dropOntoGroup(split, k("b.ts"), "g0", "center", split.focusedId, 0);
    expect(moved.groups[0].keys).toEqual(["b.ts", "a.ts"]);
  });
});
