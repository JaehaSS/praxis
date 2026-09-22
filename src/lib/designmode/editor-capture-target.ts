import type { DesignBoundingRect } from "./types";

/** 스크린샷 캡처용 — 촬영 영역이 필요하므로 bounds가 반드시 있어야 한다. */
export interface EditorCaptureTarget {
  bounds: DesignBoundingRect;
  file_path: string;
  selection_text: string | null;
  selection_start_line: number | null;
  selection_end_line: number | null;
}

/** 선택 코드 첨부(⌘L)용 — 텍스트만 쓰므로 촬영 영역이 없어도 유효하다. */
export type EditorSelectionTarget = Omit<EditorCaptureTarget, "bounds">;

/** 에디터가 보고하는 원본. bounds는 화면에서 벗어난 동안 null일 수 있다. */
type EditorTarget = EditorSelectionTarget & { bounds: DesignBoundingRect | null };

type TargetReader = () => EditorTarget | null;

const readers = new Map<number, TargetReader>();

export function registerEditorCaptureTarget(taskId: number, reader: TargetReader): () => void {
  readers.set(taskId, reader);
  return () => {
    if (readers.get(taskId) === reader) readers.delete(taskId);
  };
}

/** 촬영 영역이 없으면 캡처할 수 없다 — null로 알린다. */
export function readEditorCaptureTarget(taskId: number): EditorCaptureTarget | null {
  const target = readers.get(taskId)?.() ?? null;
  if (target === null || target.bounds === null) return null;
  return { ...target, bounds: target.bounds };
}

/** 선택 텍스트만 읽는다 — bounds 유무와 무관하게 동작한다. */
export function readEditorSelection(taskId: number): EditorSelectionTarget | null {
  const target = readers.get(taskId)?.() ?? null;
  if (target === null) return null;
  const { bounds: _bounds, ...selection } = target;
  return selection;
}
