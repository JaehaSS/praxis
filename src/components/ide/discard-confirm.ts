// 폐기 대상의 형태와 판별 — **남는 것과 사라지는 것을 이름으로** 말하기 위한 재료.
//
// 브랜치가 일치하면 폐기는 워크트리를 지우지만 브랜치는 남긴다. 지우기 전에 상태를 커밋하므로
// (`Worktree::preserve_and_retire`) 되찾을 지점이 있다. 그것을 말하지 않으면 사용자는 브랜치가
// 남는 줄도, 이름이 무엇인지도 모른다 — 내구성만 풀고 회수 가능성을 안 푼 것이 된다(설계 0056).
//
// 그 말하기를 여기서 하지는 않는다. 확인 대화가 유예 창으로 대체되면서 둘로 나뉘었다 —
// 버리기 직전에는 `removalDetail`이 유예 배너에서, 버린 뒤에는 `preservedBranchLabel`이
// 목록에서 말한다. 이 파일은 그 둘이 공유하는 대상 형태와 직접 실행 판별을 갖는다.

export interface DiscardTarget {
  repo: string;
  branch: string;
  worktree_path: string;
}

/** 직접 실행 판별 — 백엔드의 `Worktree::is_direct`와 같은 규칙(경로 == 레포 경로). */
export function isDirectRun(t: DiscardTarget): boolean {
  return t.worktree_path === t.repo;
}

/**
 * 폐기 이후 복구 단서. 브랜치가 바뀐 작업은 원래 이름의 존재를 보장하지 않는다.
 *
 * 유예 배너는 10초 동안만 보인다. 한 달 뒤 근거를 찾는 사람에게는 목록에 이름이
 * 남아 있어야 한다. 직접 실행은 만들어진 브랜치가 없으므로 아무것도 말하지 않는다.
 */
export function preservedBranchLabel(t: DiscardTarget & { state: string }): string | null {
  if (t.state !== "Discarded" || isDirectRun(t)) return null;
  return `복구 확인: ${t.worktree_path} · 작업 브랜치 ${t.branch}`;
}
