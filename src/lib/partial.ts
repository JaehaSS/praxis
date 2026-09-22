import type { DiffHunk } from "./ipc";

/**
 * hunk 부분 승인(B-2) 선택 로직 — 순수 함수로 분리해 UI(hook/컴포넌트)와 독립적으로 검증한다.
 * protected hunk는 백엔드(`partial::reject_protected`)가 "유지 선택"을 거부하므로,
 * 프론트도 처음부터 선택 후보에서 제외해 이중 방어(UI 비활성 + 서버 거부)를 완성한다.
 *
 * committed hunk도 후보에서 빠지지만 이유가 다르다. protected는 고르지 않으면 역패치로
 * 폐기되는 것이 의도된 동작이고, committed는 **아무 일도 일어나선 안 된다**.
 */

/** 기본 선택 — protected도 committed도 아닌 hunk를 유지 상태로 시작한다. */
export function defaultPartialSelection(hunks: DiffHunk[]): Set<string> {
  return new Set(hunks.filter((h) => !h.protected && !h.committed).map((h) => h.id));
}

/** 체크박스 토글 — protected·committed hunk는 선택 대상이 아니므로 무시한다(디스에이블드 UI와 이중 방어). */
export function togglePartialSelection(selected: Set<string>, hunk: DiffHunk): Set<string> {
  if (hunk.protected || hunk.committed) return selected;
  const next = new Set(selected);
  if (next.has(hunk.id)) next.delete(hunk.id);
  else next.add(hunk.id);
  return next;
}

export interface PartialSelectionSummary {
  /** 적용 후 worktree에 남을 hunk id (partialApply 호출 인자). */
  keptIds: string[];
  /** 역패치로 제거될 hunk id(협조 확인 스텝 안내 문구용). */
  discardedIds: string[];
}

/** 확인 스텝 요약 — "선택 n개 적용, 비선택 m개 hunk는 제거됩니다" 계산.
 *
 * committed hunk는 양쪽 집합에서 모두 뺀다. 유지도 폐기도 아니라서다 — 남겨 두면 확인
 * 문구가 지우지도 않을 것을 "제거됩니다"라고 세고, 백엔드(`hunks_to_revert`)와도 어긋난다. */
export function summarizePartialSelection(
  hunks: DiffHunk[],
  selected: Set<string>,
): PartialSelectionSummary {
  const keptIds: string[] = [];
  const discardedIds: string[] = [];
  for (const hunk of hunks) {
    if (hunk.committed) continue;
    (selected.has(hunk.id) ? keptIds : discardedIds).push(hunk.id);
  }
  return { keptIds, discardedIds };
}
