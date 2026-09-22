import type { EnsembleMatrix, HunkRef } from "./ipc";

/**
 * B-3 ensemble 조합 병합 — 선택 상태·배타 그룹 로직을 순수 함수로 분리한다(UI/hook과 독립 검증).
 * 배타 그룹(겹치는 hunk)은 그룹 내 최대 1개만 선택 가능 — 백엔드(`ensemble::compose`)의
 * `ExclusiveGroupViolation` 방어와 대칭인 프론트 이중 방어(PRD F-05 AC).
 */

/** 선택 키(`task_id:hunk_id`) — Set 저장/조회용 직렬화. */
export function hunkKey(ref: HunkRef): string {
  return `${ref.task_id}:${ref.hunk_id}`;
}

function parseHunkKey(key: string): HunkRef {
  const sep = key.indexOf(":");
  return { task_id: Number(key.slice(0, sep)), hunk_id: key.slice(sep + 1) };
}

/** 초기 선택 — 심판 추천 후보(winner)의 hunk를 베이스로 미리 선택해둔다(조합 전에는 winner 그대로). */
export function defaultComposeSelection(winnerTaskId: number, winnerHunkIds: string[]): Set<string> {
  return new Set(winnerHunkIds.map((hunkId) => hunkKey({ task_id: winnerTaskId, hunk_id: hunkId })));
}

/** ref가 속한 배타 그룹(있으면 그 멤버 목록, 없으면 null). */
export function exclusiveGroupOf(matrix: EnsembleMatrix, ref: HunkRef): HunkRef[] | null {
  const group = matrix.exclusive_groups.find((members) =>
    members.some((m) => m.task_id === ref.task_id && m.hunk_id === ref.hunk_id),
  );
  return group ?? null;
}

/** 체크박스 토글 — 배타 그룹에 속한 hunk를 새로 선택하면 같은 그룹의 기존 선택(다른 후보의
 *  겹치는 hunk 포함, winner의 베이스 hunk 포함)을 자동 해제한다. */
export function toggleComposeSelection(
  selected: Set<string>,
  matrix: EnsembleMatrix,
  ref: HunkRef,
): Set<string> {
  const key = hunkKey(ref);
  const next = new Set(selected);
  if (next.has(key)) {
    next.delete(key);
    return next;
  }
  const group = exclusiveGroupOf(matrix, ref);
  if (group) {
    for (const member of group) next.delete(hunkKey(member));
  }
  next.add(key);
  return next;
}

/** 선택 집합 안에 배타 그룹 위반(같은 그룹에서 2개 이상 선택)이 있으면 위반 그룹들을 반환한다.
 *  `toggleComposeSelection`을 거치면 발생하지 않지만, 제출 직전 최종 확인·초기값 검증에 쓴다. */
export function exclusiveViolations(selected: Set<string>, matrix: EnsembleMatrix): HunkRef[][] {
  return matrix.exclusive_groups.filter(
    (group) => group.filter((member) => selected.has(hunkKey(member))).length > 1,
  );
}

/** `ensembleCompose` 호출 인자 — winner 자신의 hunk는 이미 베이스에 있으므로 제외한다. */
export function composeSelections(selected: Set<string>, winnerTaskId: number): HunkRef[] {
  return Array.from(selected)
    .map(parseHunkKey)
    .filter((ref) => ref.task_id !== winnerTaskId);
}

/** 선택 요약(후보별 선택 hunk 수) — 선택 요약 바 표시용. */
export function summarizeComposeSelection(selected: Set<string>): Record<number, number> {
  const counts: Record<number, number> = {};
  for (const ref of Array.from(selected).map(parseHunkKey)) {
    counts[ref.task_id] = (counts[ref.task_id] ?? 0) + 1;
  }
  return counts;
}

/** ref를 새로 선택하면 자동 해제될(같은 배타 그룹의 기존 선택) 멤버들 — 토글 직전 경고 문구용.
 *  이미 선택된 ref를 해제하는 토글이거나 배타 그룹이 없으면 빈 배열(경고 없음). */
export function conflictingSelections(selected: Set<string>, matrix: EnsembleMatrix, ref: HunkRef): HunkRef[] {
  if (selected.has(hunkKey(ref))) return [];
  const group = exclusiveGroupOf(matrix, ref);
  if (!group) return [];
  return group
    .filter((member) => !(member.task_id === ref.task_id && member.hunk_id === ref.hunk_id))
    .filter((member) => selected.has(hunkKey(member)));
}
