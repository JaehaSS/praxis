// 파일별 "확인함" 표시 — 검토 진행 상태를 기억에 맡기지 않기 위한 로컬 상태.
// 저장하는 값은 patch 지문이다. 파일 내용이 바뀌면 지문이 달라져 확인 표시가 저절로
// 무효가 된다 — 낡은 확인 표시는 "안 본 변경"을 "본 것"으로 위장하기 때문이다.

export type ViewedState = Record<string, string>;

/** patch 내용 지문(djb2 변형). 충돌해도 최악의 결과는 확인 표시가 하나 유지되는 것이라
 *  암호학적 강도는 필요 없다. 길이를 함께 담아 같은 길이끼리의 충돌만 남긴다. */
export function fingerprint(patch: string): string {
  let hash = 5381;
  for (let i = 0; i < patch.length; i++) hash = ((hash * 33) ^ patch.charCodeAt(i)) >>> 0;
  return `${patch.length}:${hash.toString(36)}`;
}

export function isViewed(state: ViewedState, path: string, patch: string): boolean {
  return state[path] === fingerprint(patch);
}

export function markViewed(state: ViewedState, path: string, patch: string): ViewedState {
  return { ...state, [path]: fingerprint(patch) };
}

export function clearViewed(state: ViewedState, path: string): ViewedState {
  const { [path]: _removed, ...rest } = state;
  return rest;
}

export function viewedCount(state: ViewedState, files: { path: string; patch: string }[]): number {
  return files.filter((file) => isViewed(state, file.path, file.patch)).length;
}

const storageKey = (taskId: number) => `praxis:diff-viewed:${taskId}`;

export function loadViewed(taskId: number): ViewedState {
  try {
    const raw = window.localStorage.getItem(storageKey(taskId));
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    return parsed && typeof parsed === "object" ? (parsed as ViewedState) : {};
  } catch {
    return {};
  }
}

export function saveViewed(taskId: number, state: ViewedState): void {
  try {
    window.localStorage.setItem(storageKey(taskId), JSON.stringify(state));
  } catch {
    // private mode 등 storage 거부 시 현재 세션 상태만 유지한다(praxis:diff-mode와 같은 방침).
  }
}
