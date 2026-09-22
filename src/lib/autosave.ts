import type { OpenFile } from "../components/ide/EditorPane";

/** 이보다 큰 파일은 되돌리기 버퍼를 잡지 않는다 — 유예 몇 초를 위해 메모리를 크게 물지 않는다. */
export const UNDO_SIZE_LIMIT = 1024 * 1024;

export type AutosavePlan =
  | { kind: "skip" }
  | { kind: "save"; content: string; undoContent: string | null }
  | { kind: "conflict"; path: string };

/**
 * 자동 저장 판정. `diskContent`는 호출 직전 `fsRead`로 읽은 값이다.
 *
 * 충돌 기준은 `baseContent`(디스크에서 마지막으로 읽은 내용)다. `content`(편집값)와 비교하면
 * 편집한 파일은 정의상 항상 다르므로 판정이 무의미해진다 — 기존 `saveFile`의 가드가 그
 * 함정에 있고, 그래서 편집한 파일을 저장할 때마다 덮어쓰기를 묻는다.
 *
 * 충돌은 곧 에이전트가 같은 파일을 고쳤다는 뜻이므로 덮어쓰지 않는다. 시야 밖 창에서
 * `window.confirm`으로 묻는 것도 답이 아니다 — 보이지 않는 모달은 자동 저장을 택한 이유를
 * 스스로 무너뜨린다.
 */
export function planAutosave(file: OpenFile, diskContent: string): AutosavePlan {
  if (!file.dirty || file.kind !== "text") return { kind: "skip" };
  if (diskContent !== file.baseContent) return { kind: "conflict", path: file.path };
  return {
    kind: "save",
    content: file.content,
    undoContent: diskContent.length > UNDO_SIZE_LIMIT ? null : diskContent,
  };
}
