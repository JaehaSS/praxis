import type { EditorAskPayload } from "./editor-window-events";

/**
 * 선택한 코드 위에 질문 버블을 띄울지.
 *
 * 길이 상한은 여기서 보지 않는다 — `buildSelectionCapture`의 `truncateSelection`이 이미
 * 자르고 넘치면 null을 돌려준다. 두 곳에서 판정하면 규칙이 두 벌이 된다.
 */
export function shouldShowBubble(selectionText: string): boolean {
  return selectionText.trim().length > 0;
}

/** 아래 여백이 버블 높이보다 좁으면 위로 뒤집는다 — 파일 끝을 선택해도 화면 안에 남게. */
export function bubblePlacement(bottomGap: number, bubbleHeight: number): "below" | "above" {
  return bottomGap >= bubbleHeight ? "below" : "above";
}

interface AskInput {
  taskId: number;
  filePath: string;
  selectionText: string;
  startLine: number;
  endLine: number;
  question: string;
}

/** 보낼 것이 없으면 null. 빈 질문이나 빈 선택으로 세션을 깨우지 않는다. */
export function buildAskPayload(input: AskInput): EditorAskPayload | null {
  const question = input.question.trim();
  if (question.length === 0) return null;
  if (!shouldShowBubble(input.selectionText)) return null;
  return {
    task_id: input.taskId,
    file_path: input.filePath,
    start_line: input.startLine,
    end_line: input.endLine,
    selection_text: input.selectionText,
    question,
  };
}
