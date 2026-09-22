/**
 * textarea 되돌리기 스택 — DOM에 의존하지 않는 순수 자료구조.
 *
 * Edit 메뉴에서 Undo/Redo 항목을 뺀 뒤(Monaco 되돌리기를 살리려고) WKWebView의 일반
 * textarea는 ⌘Z가 죽었다 — WebKit의 폼 컨트롤 undo가 메뉴 셀렉터 경로로만 동작하기 때문.
 * 그 자리를 이 스택이 메운다.
 */

export interface TextSnapshot {
  value: string;
  /** selectionStart */
  start: number;
  /** selectionEnd */
  end: number;
}

/** 편집의 종류 — 같은 종류끼리만 한 묶음으로 되돌린다. */
export type EditKind = "insert" | "delete" | "other";

export interface UndoStack {
  /**
   * 변경 하나를 기록한다. prev = 변경 전, next = 변경 후, now = ms 타임스탬프.
   * kind를 주면 분류를 건너뛴다 — IME 조합처럼 모양만으로는 알 수 없는 편집에 쓴다.
   */
  record(prev: TextSnapshot, next: TextSnapshot, now: number, kind?: EditKind): void;
  /** 되돌릴 것이 없으면 null. current는 지금 상태(다시하기 스택에 들어간다). */
  undo(current: TextSnapshot): TextSnapshot | null;
  redo(current: TextSnapshot): TextSnapshot | null;
  canUndo(): boolean;
  canRedo(): boolean;
}

interface Options {
  /** 같은 종류의 편집을 한 묶음으로 볼 시간 간격. */
  coalesceMs?: number;
  /** 과거 스냅샷 보관 개수 상한. */
  limit?: number;
}

const DEFAULT_COALESCE_MS = 800;
const DEFAULT_LIMIT = 200;

/** 캐럿 자리에 한 글자가 들어갔으면 그 글자를, 아니면 null. */
function insertedChar(prev: TextSnapshot, next: TextSnapshot): string | null {
  if (next.value.length !== prev.value.length + 1) return null;
  const pos = next.start - 1;
  if (pos < 0) return null;
  if (prev.value.slice(0, pos) !== next.value.slice(0, pos)) return null;
  if (prev.value.slice(pos) !== next.value.slice(pos + 1)) return null;
  return next.value[pos];
}

/** 캐럿 자리에서 한 글자가 지워졌는지. */
function isSingleDelete(prev: TextSnapshot, next: TextSnapshot): boolean {
  if (prev.value.length !== next.value.length + 1) return false;
  const pos = next.start;
  if (pos < 0 || pos > next.value.length) return false;
  return (
    prev.value.slice(0, pos) === next.value.slice(0, pos) &&
    prev.value.slice(pos + 1) === next.value.slice(pos)
  );
}

function classify(prev: TextSnapshot, next: TextSnapshot): EditKind {
  // 선택 영역이 있던 편집은 치환이다 — 한 글자짜리라도 앞 타이핑과 묶지 않는다.
  if (prev.start !== prev.end) return "other";
  const ch = insertedChar(prev, next);
  // 줄바꿈 삽입은 묶지 않는다 — 줄 단위가 되돌리기의 자연스러운 경계다.
  if (ch !== null) return ch === "\n" ? "other" : "insert";
  if (isSingleDelete(prev, next)) return "delete";
  return "other";
}

export function createUndoStack(opts: Options = {}): UndoStack {
  const coalesceMs = opts.coalesceMs ?? DEFAULT_COALESCE_MS;
  const limit = opts.limit ?? DEFAULT_LIMIT;
  const past: TextSnapshot[] = [];
  const future: TextSnapshot[] = [];
  let lastKind: EditKind | null = null;
  let lastAt = 0;
  let groupOpen = false;

  const closeGroup = (): void => {
    lastKind = null;
    groupOpen = false;
  };

  const push = (snapshot: TextSnapshot): void => {
    past.push(snapshot);
    if (past.length > limit) past.shift();
  };

  const record = (
    prev: TextSnapshot,
    next: TextSnapshot,
    now: number,
    hint?: EditKind,
  ): void => {
    future.length = 0;
    const kind = hint ?? classify(prev, next);
    const merged =
      groupOpen && kind !== "other" && kind === lastKind && now - lastAt <= coalesceMs;
    // 묶이지 않는 편집만 새 스냅샷을 남긴다 — 묶음의 시작은 이미 스택에 있는 prev다.
    if (!merged) push(prev);
    lastAt = now;
    lastKind = kind;
    // 단어 단위 되돌리기: 공백까지는 앞 묶음에 넣되 그 뒤로는 새로 시작한다.
    const ch = insertedChar(prev, next);
    groupOpen = kind !== "other" && !(ch !== null && /\s/.test(ch));
  };

  const undo = (current: TextSnapshot): TextSnapshot | null => {
    closeGroup();
    const snapshot = past.pop();
    if (!snapshot) return null;
    future.push(current);
    return snapshot;
  };

  const redo = (current: TextSnapshot): TextSnapshot | null => {
    closeGroup();
    const snapshot = future.pop();
    if (!snapshot) return null;
    push(current);
    return snapshot;
  };

  return {
    record,
    undo,
    redo,
    canUndo: () => past.length > 0,
    canRedo: () => future.length > 0,
  };
}
