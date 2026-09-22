import type { LspTarget } from "./ipc";

/** 에디터 커서 위치 (Monaco 좌표 — 1-based). */
export interface CursorAt {
  path: string;
  line: number;
}

/** 점프 요청의 결말. UI는 이 셋만 처리하면 된다. */
export type GotoOutcome =
  | { kind: "jump"; target: LspTarget }
  | { kind: "choose"; targets: LspTarget[] }
  | { kind: "none" };

const idOf = (t: LspTarget) => `${t.abs_path}:${t.line}:${t.column}`;

/** 같은 위치를 두 번 가리키는 결과를 접는다 — 서버는 종종 중복을 돌려준다. */
export function dedupeTargets(targets: LspTarget[]): LspTarget[] {
  const seen = new Set<string>();
  return targets.filter((t) => {
    const id = idOf(t);
    if (seen.has(id)) return false;
    seen.add(id);
    return true;
  });
}

/** 이 결과가 커서가 이미 서 있는 곳인가 (= 선언 위에서 누른 것). */
export function isSelfTarget(target: LspTarget, at: CursorAt): boolean {
  return !target.external && target.path === at.path && target.line === at.line;
}

/**
 * JetBrains ⌘B 의미론의 핵심 판정 — 정의 결과가 제자리뿐이면 "사용처"로 넘어가야 한다.
 *
 * 선언 위에서 ⌘B를 누르면 서버는 그 선언 자신을 정의로 돌려준다. 그대로 점프하면
 * 화면이 안 움직여 고장난 것처럼 보인다. 이때 references로 폴백하는 것이 IDE의 동작이다.
 */
export function shouldFallbackToReferences(targets: LspTarget[], at: CursorAt): boolean {
  return targets.length > 0 && targets.every((t) => isSelfTarget(t, at));
}

/** 결과 목록 → UI 결말. 하나면 바로 점프, 여럿이면 고르게 한다. */
export function resolveOutcome(targets: LspTarget[]): GotoOutcome {
  const unique = dedupeTargets(targets);
  if (unique.length === 0) return { kind: "none" };
  if (unique.length === 1) return { kind: "jump", target: unique[0] };
  return { kind: "choose", targets: unique };
}

/** 결과 목록에 붙일 표시용 라벨 — 워크트리 밖이면 절대 경로를 그대로 보여준다. */
export function targetLabel(target: LspTarget): string {
  const where = target.path ?? target.abs_path;
  return `${where}:${target.line}`;
}
