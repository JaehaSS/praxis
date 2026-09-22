// 컴포저 포커스 요청 in-memory pub-sub. 첨부를 만든 쪽(단축키 핸들러)과 입력창은 서로 모르는
// 컴포넌트라 ref를 넘길 수 없다 — designmode/store.ts의 캡처 pub-sub과 같은 방식으로 taskId 격리.

type Listener = () => void;

const listeners = new Map<number, Set<Listener>>();

/** 해당 작업의 컴포저 입력창에 포커스를 요청한다. 구독자가 없으면 아무 일도 없다. */
export function requestComposerFocus(taskId: number): void {
  for (const listener of listeners.get(taskId) ?? []) listener();
}

/** 반환된 함수를 언마운트 시 호출해 구독을 해제한다. */
export function subscribeComposerFocus(taskId: number, listener: Listener): () => void {
  const set = listeners.get(taskId) ?? new Set();
  set.add(listener);
  listeners.set(taskId, set);
  return () => set.delete(listener);
}
