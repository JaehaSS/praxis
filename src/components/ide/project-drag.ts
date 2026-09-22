/**
 * 사이드바 프로젝트·그룹을 끌어다 놓는 것의 운반 형식과 받아들임 판정.
 *
 * `editor-drag.ts`와 같은 구조다 — 끄는 쪽과 받는 쪽이 각자 문자열을 적으면 한쪽만 고쳤을 때
 * 드롭이 조용히 죽는다. 형식이 안 맞아도 dataTransfer는 오류를 내지 않기 때문이다.
 */

/** 프로젝트 박스. 값은 repo 절대경로. */
export const PROJECT_DRAG_MIME = "application/x-praxis-project";

/** 그룹 헤더. 값은 group id. */
export const PROJECT_GROUP_DRAG_MIME = "application/x-praxis-project-group";

export type ProjectDragKind = "project" | "group";

/**
 * 지금 끌고 있는 것이 우리가 아는 것인가.
 *
 * `dragover`에서는 dataTransfer가 보호 모드라 **값이 아니라 타입만 보인다**. 강조 여부는 이
 * 함수가 정하고 실제 값은 `drop`에서 꺼낸다(`editor-drag.ts:draggedKind`와 같은 제약).
 */
export function draggedProjectKind(
  types: readonly string[] | DOMStringList | undefined,
): ProjectDragKind | null {
  if (types == null) return null;
  const list = Array.from(types as ArrayLike<string>);
  if (list.includes(PROJECT_DRAG_MIME)) return "project";
  if (list.includes(PROJECT_GROUP_DRAG_MIME)) return "group";
  return null;
}

/**
 * 이 구획이 지금의 드래그를 받는가. `target`이 null이면 미소속 구획이다.
 *
 * 프로젝트 전용 판정이다. 그룹 이동은 `canMoveGroup`이 부모·후손 관계를 확인한다.
 * 이미 그 구획에 있는 프로젝트는 놓아도 바뀌는 것이 없으므로 강조하지 않는다.
 */
export const acceptsProjectDrop = (
  kind: ProjectDragKind | null,
  currentGroupId: string | null,
  target: string | null,
): boolean => kind === "project" && currentGroupId !== target;

/** `DESIGN.md` components.DropTarget.zone — 드롭하면 안에 들어가는 대상. */
export const DROP_ZONE_CLASS = "border-2 border-primary bg-primary/20";

/** `DESIGN.md` components.DropTarget.caret — 드롭하면 사이에 끼는 자리. */
export const DROP_CARET_CLASS = "h-0.5 rounded bg-primary";
