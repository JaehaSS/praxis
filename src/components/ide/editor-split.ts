/**
 * 에디터 분할 배치 — 순수 모델.
 *
 * 파일의 내용·dirty·저장 충돌은 `useWorkspaceFiles` 한 벌이 소유한다. 여기가 다루는 것은
 * **어느 그룹이 어떤 탭을 들고 있는가**뿐이다. 갈라 두지 않으면 같은 파일이 그룹마다 다른
 * dirty를 갖게 되고, 어느 쪽 편집이 디스크로 나가는지 사용자가 판단해야 한다.
 *
 * 그래서 같은 경로가 두 그룹에 동시에 있는 것은 정상이다 — 배치의 복제이지 내용의 복제가
 * 아니다. 분할이 "지금 보던 파일을 옆에도 띄운다"로 시작하는 이상 피할 수 없는 상태이기도 하다.
 *
 * 격자(2×2)는 범위 밖이다. 축이 배치 전역 하나여서 방향을 바꾸면 모든 그룹이 함께 돈다 —
 * 트리를 들이면 "어느 경계를 끄는 중인가"를 사용자가 추적해야 하고, 그 값이 여는 파일 수에
 * 비해 크지 않다.
 *
 * 탭을 가리키는 것은 경로가 아니라 `TabKey`다 — 같은 파일이 파일 탭과 diff 탭으로 함께 열릴 수
 * 있어 경로로는 둘을 가르지 못한다(`lib/tab-key`).
 */

import type { TabKey } from "../../lib/tab-key";

/** 그룹이 늘어서는 축. `row`=좌우(오른쪽으로 분할) · `column`=상하(아래로 분할). */
export type SplitAxis = "row" | "column";

export interface EditorGroup {
  id: string;
  /** 이 그룹의 탭 순서. 전역 openFiles의 부분집합이다. */
  keys: TabKey[];
  activeKey: TabKey | null;
  /**
   * 축 방향 크기 비중 — flex-grow로 그대로 나간다. 비율만 뜻하므로 그룹이 사라져도
   * 남은 값을 다시 나눌 필요가 없다(1:1에서 하나가 빠지면 남은 하나가 전부를 갖는다).
   */
  size: number;
}

export interface SplitLayout {
  groups: EditorGroup[];
  axis: SplitAxis;
  focusedId: string;
  /** 다음 그룹 id 번호. 번호를 재사용하면 React key가 겹쳐 새 그룹이 옛 그룹의 DOM을 물려받는다. */
  seq: number;
}

/**
 * 그룹 수 상한. 코드 열은 창의 절반 남짓이고, 넷으로 쪼개면 어느 칸도 한 줄을 온전히 못 담는다.
 * 상한에 닿아도 축 전환은 여전히 유효한 응답이라 분할 요청을 통째로 버리지는 않는다.
 */
export const MAX_GROUPS = 3;

/** 그룹 하나가 쓸모를 유지하는 최소 픽셀. 경계를 끌 때 양쪽 모두 이만큼은 남긴다. */
export const MIN_GROUP_PX = 180;

/** 탭을 떨군 자리. `center`는 그 칸 안으로 옮기고, 네 방향은 그 자리에 새 칸을 만든다. */
export type DropEdge = "left" | "right" | "top" | "bottom" | "center";

/** 방향이 곧 축이다 — 좌우로 떨구면 가로 배치, 위아래로 떨구면 세로 배치. */
const EDGE_AXIS: Record<Exclude<DropEdge, "center">, SplitAxis> = {
  left: "row",
  right: "row",
  top: "column",
  bottom: "column",
};

const groupOf = (layout: SplitLayout, id: string): EditorGroup | undefined =>
  layout.groups.find((g) => g.id === id);

/**
 * 비중의 합을 그룹 수로 맞춘다 — 비율은 그대로 두고 **총합만** 바로잡는다.
 *
 * `size`는 flex-grow로 그대로 나가는데, flex-grow의 합이 1보다 작으면 브라우저는 남은 공간을
 * 그 비율만큼**만** 나눠 준다. 0.5짜리 칸 하나만 남으면 컨테이너의 절반이 빈 채로 남는다는 뜻이다.
 * 분할은 자기 몫을 반으로 쪼개므로(`splitFocused`) 칸이 사라질 때마다 합이 줄고, 파일을 전부
 * 닫았다가 다시 열면 "옛 분할의 좁은 폭을 물려받은" 칸이 나타난다.
 *
 * 그래서 그룹 목록이 바뀌는 모든 자리에서 한 번씩 통과시킨다. 합을 1이 아니라 **그룹 수**로
 * 맞추는 이유는 값이 1 언저리에 머물러 읽기 쉽기 때문이다(칸 하나면 정확히 1).
 * 바뀔 것이 없으면 받은 배열을 그대로 돌려준다 — `sameLayout`이 값 비교를 하므로 참조를
 * 새로 만들어도 틀리지는 않지만, 만들지 않는 편이 낫다.
 */
function normalizeSizes(groups: EditorGroup[]): EditorGroup[] {
  if (groups.length === 0) return groups;
  const sum = groups.reduce((total, g) => total + Math.max(g.size, 0), 0);
  if (sum <= 0) return groups.map((g) => ({ ...g, size: 1 }));
  const factor = groups.length / sum;
  if (Math.abs(factor - 1) < 1e-9) return groups;
  return groups.map((g) => ({ ...g, size: Math.max(g.size, 0) * factor }));
}

/** 모든 칸을 같은 폭으로. 경계를 더블클릭하면 여기로 온다 — 되돌릴 지점이 하나는 있어야 한다. */
export function evenSizes(layout: SplitLayout): SplitLayout {
  if (layout.groups.every((g) => g.size === 1)) return layout;
  return { ...layout, groups: layout.groups.map((g) => ({ ...g, size: 1 })) };
}

/** 포커스된 그룹. 포커스가 사라진 배치는 만들지 않지만, 읽는 쪽이 null을 다루지 않게 첫 그룹으로 떨어진다. */
export function focusedGroup(layout: SplitLayout): EditorGroup {
  return groupOf(layout, layout.focusedId) ?? layout.groups[0];
}

/** 이 탭을 아직 어느 그룹이라도 들고 있는가 — 전역에서 닫을지 판단하는 근거. */
export function isOpenInAnyGroup(layout: SplitLayout, key: TabKey): boolean {
  return layout.groups.some((g) => g.keys.includes(key));
}

export function createLayout(keys: TabKey[] = [], activeKey: TabKey | null = null): SplitLayout {
  const active =
    activeKey != null && keys.includes(activeKey) ? activeKey : (keys[keys.length - 1] ?? null);
  return {
    groups: [{ id: "g0", keys: [...keys], activeKey: active, size: 1 }],
    axis: "row",
    focusedId: "g0",
    seq: 1,
  };
}

/** 탭 하나를 닫은 뒤 활성으로 승계할 키 — 오른쪽 이웃, 없으면 왼쪽. */
function heirKey(keys: TabKey[], closed: TabKey): TabKey | null {
  const index = keys.indexOf(closed);
  const rest = keys.filter((p) => p !== closed);
  if (rest.length === 0) return null;
  return rest[Math.min(index, rest.length - 1)] ?? null;
}

/** 그룹을 배치에서 뺀다. 탭 이관은 하지 않는다 — 부르는 쪽이 이미 처리했거나 빈 그룹이다. */
function dropGroup(layout: SplitLayout, id: string): SplitLayout {
  if (layout.groups.length <= 1) return layout;
  const index = layout.groups.findIndex((g) => g.id === id);
  if (index < 0) return layout;
  const heirId = layout.groups[index === 0 ? 1 : index - 1].id;
  return {
    ...layout,
    groups: normalizeSizes(layout.groups.filter((g) => g.id !== id)),
    focusedId: layout.focusedId === id ? heirId : layout.focusedId,
  };
}

/**
 * 포커스된 그룹을 쪼갠다. 새 그룹은 **지금 보던 파일을 들고** 태어나고 포커스를 가져간다.
 *
 * 빈 칸으로 열면 분할 직후 무엇을 볼지 다시 골라야 하고, 반대로 원본에서 파일을 옮겨 오면
 * 왼쪽이 비어 "분할했더니 보던 게 사라졌다"가 된다. 복제해 두면 트리에서 다음 파일을 여는
 * 순간(새 그룹이 포커스라 그리로 열린다) 자연히 좌우가 다른 파일이 된다.
 */
export function splitFocused(layout: SplitLayout, axis: SplitAxis): SplitLayout {
  const source = focusedGroup(layout);
  if (layout.groups.length >= MAX_GROUPS) {
    return layout.axis === axis ? layout : { ...layout, axis };
  }
  // 띄울 것이 없으면 쪼갤 것도 없다 — 빈 그룹은 sync가 곧바로 걷어 가므로 배치만 흔들린다.
  if (source.activeKey == null) {
    return layout.axis === axis ? layout : { ...layout, axis };
  }
  const created: EditorGroup = {
    id: `g${layout.seq}`,
    keys: [source.activeKey],
    activeKey: source.activeKey,
    // 자기 몫에서 떼어 준다 — 옆 그룹의 폭이 따라 움직이면 "왜 저기가 좁아졌지"가 된다.
    size: source.size / 2,
  };
  return {
    groups: normalizeSizes(
      layout.groups.flatMap((g) =>
        g.id === source.id ? [{ ...g, size: g.size / 2 }, created] : [g],
      ),
    ),
    axis,
    focusedId: created.id,
    seq: layout.seq + 1,
  };
}

/**
 * 이 자리에 떨굴 수 있는가 — 드래그 중 강조를 켤지 판단하는 근거.
 *
 * 반응하지 않을 자리에 강조를 그리면 사용자는 놓아 본 뒤에야 안 된다는 것을 안다.
 * 그래서 판정을 `dropOntoGroup`과 한 벌로 두고, 그리는 쪽이 미리 물어본다.
 */
export function canDropOntoGroup(
  layout: SplitLayout,
  key: TabKey,
  targetGroupId: string,
  edge: DropEdge,
  sourceGroupId: string | null,
  at?: number,
): boolean {
  const target = groupOf(layout, targetGroupId);
  if (target == null) return false;
  if (edge === "center") {
    // 자리를 지정해 떨구는 것은 순서를 바꾸겠다는 뜻이라 이미 띄운 파일이라도 할 일이 있다.
    if (at != null) return target.keys.includes(key) || sourceGroupId !== targetGroupId;
    // 자리 지정이 없으면 "이 칸에 띄워 달라"뿐이다 — 이미 띄우고 있으면 옮길 곳이 없다.
    return !(sourceGroupId === targetGroupId && target.activeKey === key);
  }
  const source = sourceGroupId == null ? null : groupOf(layout, sourceGroupId);
  const soleTab = source != null && source.keys.length === 1 && source.keys[0] === key;
  // 자기 칸의 유일한 탭을 자기 가장자리에 떨구면, 떠난 자리를 새 칸이 그대로 메워 같은 배치가 된다.
  if (soleTab && source.id === targetGroupId) return false;
  // 원본이 그 탭 하나뿐이면 떠난 칸은 접힌다 — 칸 수가 늘지 않으므로 상한에 걸리지 않는다.
  const vacates = soleTab && layout.groups.length > 1;
  return layout.groups.length + 1 - (vacates ? 1 : 0) <= MAX_GROUPS;
}

/**
 * 탭을 다른 칸(또는 그 가장자리)에 떨군다 — 분할 버튼을 거치지 않는 두 번째 분할 경로.
 *
 * **복제가 아니라 이동이다.** 버튼 분할(`splitFocused`)이 보던 파일을 복제하는 것과 반대인데,
 * 손으로 탭을 집어 옮기는 동작에서 원본이 남으면 "옮겼는데 그대로 있다"가 되기 때문이다.
 * 원본 칸이 그 탭 하나뿐이었다면 칸째 접힌다(`closeKeyInGroup`이 이미 그렇게 한다).
 *
 * 축이 배치 전역 하나라는 제약은 그대로다 — 세로로 나뉜 상태에서 좌우 가장자리에 떨구면
 * 배치 전체가 가로로 돈다. 격자를 만들지 않는 대신 방향은 마지막 조작을 따른다.
 */
export function dropOntoGroup(
  layout: SplitLayout,
  key: TabKey,
  targetGroupId: string,
  edge: DropEdge,
  sourceGroupId: string | null,
  /** 탭 바에 떨궜을 때의 삽입 자리(`center` 전용). 없으면 끝에 붙는다. */
  at?: number,
): SplitLayout {
  if (!canDropOntoGroup(layout, key, targetGroupId, edge, sourceGroupId, at)) return layout;

  // 원본에서 뗀다. 마지막 탭이었으면 칸이 사라지므로 대상 자리는 이 뒤에 다시 찾는다.
  // 제자리 center 드롭만 예외다 — 뗐다 붙이면 탭 순서가 끝으로 밀린다.
  const detach = sourceGroupId != null && !(edge === "center" && sourceGroupId === targetGroupId);
  const detached =
    detach && sourceGroupId != null ? closeKeyInGroup(layout, sourceGroupId, key) : layout;

  if (edge === "center") return activateInGroup(detached, targetGroupId, key, at);

  const index = detached.groups.findIndex((g) => g.id === targetGroupId);
  if (index < 0) return layout;
  const host = detached.groups[index];
  const created: EditorGroup = {
    id: `g${detached.seq}`,
    keys: [key],
    activeKey: key,
    // 새 칸의 몫은 받아 준 칸에서만 나온다 — 옆 칸의 폭이 따라 움직이면 원인이 안 보인다.
    size: host.size / 2,
  };
  const before = edge === "left" || edge === "top";
  const halved: EditorGroup = { ...host, size: host.size / 2 };
  return {
    ...detached,
    groups: normalizeSizes(
      detached.groups.flatMap((g, i) =>
        i !== index ? [g] : before ? [created, halved] : [halved, created],
      ),
    ),
    axis: EDGE_AXIS[edge],
    focusedId: created.id,
    seq: detached.seq + 1,
  };
}

/**
 * 그룹을 접는다. 들고 있던 탭은 이웃으로 넘어간다 — 파일을 닫지 않는다.
 *
 * 그룹 하나를 접는 것이 저장 안 된 편집 여럿을 한 번에 버리는 일이 되면, 사용자는 접기 전에
 * 매번 무엇이 dirty인지 확인해야 한다. 이관은 그 확인을 없앤다.
 */
export function closeGroup(layout: SplitLayout, id: string): SplitLayout {
  if (layout.groups.length <= 1) return layout;
  const index = layout.groups.findIndex((g) => g.id === id);
  if (index < 0) return layout;
  const closing = layout.groups[index];
  const heirId = layout.groups[index === 0 ? 1 : index - 1].id;
  const merged: SplitLayout = {
    ...layout,
    groups: layout.groups.map((g) =>
      g.id === heirId
        ? {
            ...g,
            keys: [...g.keys, ...closing.keys.filter((p) => !g.keys.includes(p))],
            activeKey: closing.activeKey ?? g.activeKey,
          }
        : g,
    ),
  };
  return dropGroup(merged, id);
}

export function focusGroup(layout: SplitLayout, id: string): SplitLayout {
  if (layout.focusedId === id || groupOf(layout, id) == null) return layout;
  return { ...layout, focusedId: id };
}

/**
 * 탭 하나를 목록의 `at` 자리에 끼운다 — 이미 있으면 옮기고, 없으면 새로 넣는다.
 *
 * 옮길 때 자기 자신을 먼저 빼고 세는 것이 중요하다. 빼지 않고 인덱스를 재면 오른쪽으로
 * 한 칸 옮기는 동작이 제자리걸음이 된다(자기가 차지한 자리만큼 목표가 밀린다).
 */
function placeAt(keys: TabKey[], key: TabKey, at: number): TabKey[] {
  const rest = keys.filter((p) => p !== key);
  const index = Math.max(0, Math.min(at, rest.length));
  return [...rest.slice(0, index), key, ...rest.slice(index)];
}

/**
 * 그 그룹에서 이 파일을 띄우고 포커스를 옮긴다. 아직 없는 탭이면 끝에 붙인다.
 *
 * `at`을 주면 그 자리에 끼운다 — 탭을 끌어 순서를 바꾸는 경로다. ⌘1‥⌘9가 탭 순서를 그대로
 * 번호로 쓰므로, 순서를 손으로 정할 수 있다는 것은 번호를 손으로 정할 수 있다는 뜻이다.
 */
export function activateInGroup(
  layout: SplitLayout,
  id: string,
  key: TabKey,
  at?: number,
): SplitLayout {
  const group = groupOf(layout, id);
  if (group == null) return layout;
  const keys = at == null ? group.keys : placeAt(group.keys, key, at);
  const settled =
    layout.focusedId === id &&
    group.activeKey === key &&
    group.keys.includes(key) &&
    keys.length === group.keys.length &&
    keys.every((p, i) => p === group.keys[i]);
  if (settled) return layout;
  return {
    ...layout,
    focusedId: id,
    groups: layout.groups.map((g) =>
      g.id === id
        ? {
            ...g,
            keys: at == null ? (g.keys.includes(key) ? g.keys : [...g.keys, key]) : keys,
            activeKey: key,
          }
        : g,
    ),
  };
}

/**
 * 그 그룹에서만 탭을 닫는다. 다른 그룹이 같은 파일을 들고 있으면 전역에서는 열린 채로 남는다 —
 * 전역 닫기 여부는 `isOpenInAnyGroup`으로 결과 배치에 물어본다.
 *
 * 마지막 탭을 닫으면 그룹째 사라진다. 빈 칸이 자리를 붙잡고 있으면 분할이 이득이 아니다.
 */
export function closeKeyInGroup(layout: SplitLayout, id: string, key: TabKey): SplitLayout {
  const group = groupOf(layout, id);
  if (group == null || !group.keys.includes(key)) return layout;
  const keys = group.keys.filter((p) => p !== key);
  if (keys.length === 0 && layout.groups.length > 1) return dropGroup(layout, id);
  const activeKey = group.activeKey === key ? heirKey(group.keys, key) : group.activeKey;
  return {
    ...layout,
    groups: layout.groups.map((g) => (g.id === id ? { ...g, keys, activeKey } : g)),
  };
}

/**
 * 경계 `index`(왼쪽/위쪽 그룹의 자리)를 픽셀만큼 민다.
 *
 * 비중은 컨테이너 크기를 모르므로 픽셀 상한을 여기서만 안다 — `totalPx`를 받아 그때그때 환산한다.
 * 드래그 중 매 프레임 부르되 **시작 시점의 배치**에 절대 델타를 적용해야 누적 오차가 없다.
 */
export function resizeBoundary(
  layout: SplitLayout,
  index: number,
  deltaPx: number,
  totalPx: number,
): SplitLayout {
  const a = layout.groups[index];
  const b = layout.groups[index + 1];
  if (a == null || b == null || totalPx <= 0) return layout;
  const unitSum = layout.groups.reduce((sum, g) => sum + g.size, 0);
  if (unitSum <= 0) return layout;
  const pxPerUnit = totalPx / unitSum;
  const minUnit = MIN_GROUP_PX / pxPerUnit;
  const span = a.size + b.size;
  // 둘이 합쳐도 최소 폭 두 개를 못 채우면 어디로 끌어도 배치가 나아지지 않는다.
  if (span < minUnit * 2) return layout;
  const nextA = Math.min(span - minUnit, Math.max(minUnit, a.size + deltaPx / pxPerUnit));
  if (nextA === a.size) return layout;
  return {
    ...layout,
    groups: layout.groups.map((g, i) =>
      i === index ? { ...g, size: nextA } : i === index + 1 ? { ...g, size: span - nextA } : g,
    ),
  };
}

function sameLayout(a: SplitLayout, b: SplitLayout): boolean {
  if (a === b) return true;
  if (a.axis !== b.axis || a.focusedId !== b.focusedId || a.seq !== b.seq) return false;
  if (a.groups.length !== b.groups.length) return false;
  return a.groups.every((g, i) => {
    const other = b.groups[i];
    return (
      g.id === other.id &&
      g.activeKey === other.activeKey &&
      g.size === other.size &&
      g.keys.length === other.keys.length &&
      g.keys.every((p, j) => p === other.keys[j])
    );
  });
}

/** 어느 탭이 훑어보기이고 어느 탭이 저장 안 됐는가 — 프리뷰 자리 판정에 필요한 전부다. */
export interface TabMarks {
  preview?: ReadonlySet<TabKey>;
  dirty?: ReadonlySet<TabKey>;
}

export interface SyncResult {
  layout: SplitLayout;
  /** 프리뷰 자리를 새 탭에 내주고 어느 칸에도 남지 않은 탭. 부르는 쪽이 전역에서 닫는다. */
  evicted: TabKey[];
}

const EMPTY_KEYS: ReadonlySet<TabKey> = new Set<TabKey>();

/** 명시적으로 고른 프리뷰 탭을 한 칸에 앉힌다 — 일반 동기화의 소유자 포커스 규칙은 건드리지 않는다. */
export function activatePreviewInGroup(
  layout: SplitLayout,
  groupId: string,
  key: TabKey,
  marks: TabMarks,
): SyncResult {
  const group = groupOf(layout, groupId);
  const preview = marks.preview ?? EMPTY_KEYS;
  const dirty = marks.dirty ?? EMPTY_KEYS;
  const replaced = group?.keys.find((item) => item !== key && preview.has(item) && !dirty.has(item));
  if (group == null || !preview.has(key) || replaced == null) return { layout: activateInGroup(layout, groupId, key), evicted: [] };
  const keys = group.keys.includes(key)
    ? group.keys.filter((item) => item !== replaced)
    : group.keys.map((item) => item === replaced ? key : item);
  const next: SplitLayout = {
    ...layout,
    focusedId: groupId,
    groups: layout.groups.map((item) => item.id === groupId ? { ...item, keys, activeKey: key } : item),
  };
  return { layout: next, evicted: next.groups.some((item) => item.keys.includes(replaced)) ? [] : [replaced] };
}

/**
 * 포커스된 칸의 프리뷰 자리에 새 프리뷰 탭을 앉힌다 — **프리뷰 자리는 칸마다 하나**다.
 *
 * 자리를 전역 목록에서 교체하면 분할된 칸이 순간 비고 `syncLayout`이 그 칸을 걷어 간다.
 * 그래서 파일 층은 표시만 붙이고, 자리를 바꾸는 것은 배치인 여기다(ADR 0189).
 *
 * 편집 중인(dirty) 프리뷰는 비켜 주지 않고, 자리는 **그 자리에** 유지한다(원장 #339).
 */
function seatPreview(
  groups: EditorGroup[],
  focusedId: string,
  fresh: TabKey[],
  marks: TabMarks,
): { groups: EditorGroup[]; evicted: TabKey[]; rest: TabKey[] } {
  const preview = marks.preview ?? EMPTY_KEYS;
  const dirty = marks.dirty ?? EMPTY_KEYS;
  let seated = groups;
  const evicted: TabKey[] = [];
  const rest: TabKey[] = [];
  for (const key of fresh) {
    if (!preview.has(key)) {
      rest.push(key);
      continue;
    }
    const focused = seated.find((g) => g.id === focusedId);
    const at = focused?.keys.findIndex((item) => preview.has(item) && !dirty.has(item)) ?? -1;
    if (focused == null || at < 0) {
      seated = seated.map((g) => g.id === focusedId ? { ...g, keys: [...g.keys, key] } : g);
      continue;
    }
    const replaced = focused.keys[at];
    evicted.push(replaced);
    seated = seated.map((g) =>
      g.id === focusedId
        ? {
            ...g,
            keys: g.keys.map((item, i) => (i === at ? key : item)),
            activeKey: g.activeKey === replaced ? key : g.activeKey,
          }
        : g,
    );
  }
  return { groups: seated, evicted, rest };
}

/**
 * 전역 파일 상태(열린 목록·활성 탭)에 배치를 맞춘다.
 *
 * 트리 클릭·정의 이동·팝아웃 복원은 전부 `useWorkspaceFiles`를 거쳐 오지 파일 그룹을 모른다.
 * 그 한 방향을 여기서 흡수한다 — 배치가 파일 열기 경로마다 손을 뻗으면 새 진입로가 생길
 * 때마다 배치도 함께 고쳐야 한다.
 *
 * 바뀐 것이 없으면 **받은 배치 참조를 그대로 돌려준다**. 이 함수는 effect에서 매 렌더 불리므로
 * 새 객체를 만들면 그 자체가 다음 렌더를 부른다.
 */
export function syncLayout(
  layout: SplitLayout,
  openKeys: TabKey[],
  activeKey: TabKey | null,
  marks: TabMarks = {},
): SyncResult {
  const open = new Set(openKeys);

  // 1) 전역에서 닫힌 파일은 모든 그룹에서 사라진다.
  let groups: EditorGroup[] = layout.groups.map((g) => {
    const keys = g.keys.filter((p) => open.has(p));
    const active =
      g.activeKey != null && keys.includes(g.activeKey)
        ? g.activeKey
        : (keys[keys.length - 1] ?? null);
    return { ...g, keys, activeKey: active };
  });
  let focusedId = layout.focusedId;

  // 2) 비어 버린 그룹은 자리를 반납한다. 마지막 하나는 비어도 남는다 — 그릴 면은 있어야 한다.
  if (groups.length > 1 && groups.some((g) => g.keys.length === 0)) {
    const kept = groups.filter((g) => g.keys.length > 0);
    groups = kept.length > 0 ? kept : [groups[0]];
    if (!groups.some((g) => g.id === focusedId)) focusedId = groups[0].id;
  }

  // 3) 어느 그룹에도 없는 새 파일은 포커스된 그룹으로 들어간다. 새 탭이 프리뷰면 그 칸의
  //    프리뷰 자리를 물려받는다 — 다른 칸은 건드리지 않는다.
  const placed = new Set(groups.flatMap((g) => g.keys));
  const fresh = openKeys.filter((p) => !placed.has(p));
  const evicted: TabKey[] = [];
  if (fresh.length > 0) {
    const seat = seatPreview(groups, focusedId, fresh, marks);
    groups = seat.groups.map((g) =>
      g.id === focusedId
        ? {
            ...g,
            keys: [...g.keys, ...seat.rest],
            activeKey:
              activeKey != null && fresh.includes(activeKey)
                ? activeKey
                : (g.activeKey ?? fresh[fresh.length - 1]),
          }
        : g,
    );
    // 옆 칸이 아직 들고 있으면 전역에서는 열린 채로 둔다(`closeInGroup`과 같은 규칙).
    for (const key of seat.evicted) {
      if (!groups.some((g) => g.keys.includes(key))) evicted.push(key);
    }
  }

  // 4) 전역 활성 탭을 포커스된 그룹에 반영한다. 그 그룹이 안 들고 있으면 들고 있는 쪽으로
  //    포커스가 넘어간다 — 정의 이동이 이미 열린 탭으로 착지할 때의 경로다.
  if (activeKey != null) {
    const focused = groups.find((g) => g.id === focusedId);
    if (focused != null && focused.activeKey !== activeKey) {
      const owner = focused.keys.includes(activeKey)
        ? focused
        : groups.find((g) => g.keys.includes(activeKey));
      if (owner != null) {
        focusedId = owner.id;
        groups = groups.map((g) => (g.id === owner.id ? { ...g, activeKey } : g));
      }
    }
  }

  // 5) 칸이 줄었다면 비중의 합도 줄어 있다. 여기서 되돌리지 않으면 파일을 전부 닫았다가
  //    다시 연 칸이 옛 분할의 좁은 폭을 물려받는다(`normalizeSizes`).
  const draft: SplitLayout = { ...layout, groups: normalizeSizes(groups), focusedId };
  return { layout: sameLayout(layout, draft) ? layout : draft, evicted };
}
