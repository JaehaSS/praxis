/**
 * 프리뷰 탭 — 트리를 한 번 클릭해서 연 파일은 **칸마다 자리 하나를 돌려 쓴다**.
 *
 * 없으면 파일을 훑어보는 동안 탭이 무한정 쌓인다. 훑어보기와 붙들어 두기는 서로 다른 의도인데
 * 탭 하나로는 그 둘이 구분되지 않기 때문이다. 그래서 의도를 손동작으로 가른다:
 * 한 번 클릭은 훑어보기(프리뷰), 더블클릭이나 편집은 붙들기(고정).
 *
 * 익숙하지 않으면 성가실 수 있는 동작이라 **끌 수 있게 해 두었다**(`previewTabsEnabled`).
 * 끄면 열기 경로가 예전 그대로가 된다 — 이 모듈을 아무도 부르지 않는 상태와 같다.
 *
 * 자리를 물려주는 일 자체는 여기가 아니라 배치가 한다(`editor-split.ts`의 `syncLayout`) —
 * 파일 층이 전역 목록에서 제자리 교체를 하면 분할된 칸이 순간 비어 접힌다(ADR 0189).
 */

const KEY = "praxis:preview-tabs";

/** 기본은 켬. 저장된 값이 없거나 storage가 막혀 있으면 이 값이다. */
export function previewTabsEnabled(): boolean {
  try {
    return window.localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

export function setPreviewTabsEnabled(on: boolean): void {
  try {
    window.localStorage.setItem(KEY, on ? "on" : "off");
  } catch {
    // private mode 등 storage 거부 — 이번 세션에만 적용된다(praxis:diff-viewed와 같은 방침).
  }
}
