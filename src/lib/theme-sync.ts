// 창 간 테마 동기화.
//
// 에디터 창은 앱 부팅 때 한 번 만들어지고(`tauri.conf.json`의 `visible: false`) 팝인해도
// 웹뷰가 살아 있다(`editor_window_hide`는 close가 아니라 hide다). 그래서 부팅 이후의 테마
// 변경은 이벤트로 건네주지 않으면 그 창에 영영 닿지 않는다 — 창은 부팅 시점 테마에 고정된다.
//
// Tauri 왕복은 여기가 소유한다. `themes.ts`는 부팅 스크립트와 보조 창도 쓰는 동기·무IO
// 모듈로 남아야 하기 때문이다(`theme-files.ts` 헤더와 같은 이유).

import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { EDITOR_READY_EVENT, EDITOR_WINDOW_LABEL, PROJECT_EDITOR_READY_EVENT } from "./editor-window-events";
import { adoptTheme, getActiveTheme, subscribeTheme, type Theme } from "./themes";

/** 메인 → 에디터. 활성 테마가 바뀔 때마다, 그리고 창이 준비를 알릴 때마다 간다. */
export const THEME_CHANGED_EVENT = "theme://changed";

/**
 * id가 아니라 파생 결과를 통째로 싣는다.
 *
 * id만 보내면 받는 창이 자기 레지스트리에서 찾아야 하는데, 커스텀 테마는 파일 로드 타이밍에
 * 걸리고 편집 드래프트는 어느 레지스트리에도 없다 — 둘 다 `getTheme` 폴백에 걸려 조용히 기본
 * 테마로 떨어진다. `Theme`은 순수 데이터라 그대로 실리고, 파생을 두 번 돌리지 않으니 두 창의
 * 색이 어긋날 여지도 없다.
 */
export type ThemePayload = Theme;

/**
 * 메인 창 쪽 — 활성 테마를 에디터 창으로 계속 흘려보낸다.
 *
 * 창이 숨어 있어도 보낸다. 팝아웃 순간에 맞춰 보내면 숨어 있던 동안의 변경이 창을 꺼낸 뒤
 * 한 프레임 늦게 반영된다. 받는 쪽이 없어 emit이 실패하는 것은 정상이므로 삼킨다.
 */
export function startThemeBroadcast(): () => void {
  const projectLabels = new Set<string>();
  const send = () => {
    void emitTo(EDITOR_WINDOW_LABEL, THEME_CHANGED_EVENT, getActiveTheme()).catch(() => {});
    for (const label of projectLabels) {
      void emitTo(label, THEME_CHANGED_EVENT, getActiveTheme()).catch(() => projectLabels.delete(label));
    }
  };
  const unsubscribe = subscribeTheme(send);
  // 창이 먼저 떠 있는 경우(dev 새로고침·메인 창만 리로드)를 위한 핸드셰이크. 창은 마운트할 때
  // `editor://ready`를 보내므로, 그 시점의 현재 값을 다시 준다.
  const ready = listen(EDITOR_READY_EVENT, send).catch((): UnlistenFn => () => {});
  const projectReady = listen<string>(PROJECT_EDITOR_READY_EVENT, ({ payload }) => {
    if (!payload.startsWith("project-editor-")) return;
    projectLabels.add(payload);
    void emitTo(payload, THEME_CHANGED_EVENT, getActiveTheme()).catch(() => {});
  }).catch((): UnlistenFn => () => {});
  send();
  return () => {
    unsubscribe();
    void ready.then((un) => un());
    void projectReady.then((un) => un());
  };
}

/**
 * 에디터 창 쪽 — 받은 테마를 그대로 입는다.
 *
 * 저장하지 않는다. localStorage는 두 창이 공유하고 정본은 테마를 고른 메인 창이 이미 썼다.
 * 여기서 또 쓰면 편집 드래프트 id가 새어 나가 다음 부팅이 그 임시 테마를 되살리려 한다.
 */
export function startThemeFollower(): () => void {
  const un = listen<ThemePayload>(THEME_CHANGED_EVENT, ({ payload }) => {
    adoptTheme(payload);
  }).catch((): UnlistenFn => () => {});
  return () => {
    void un.then((f) => f());
  };
}
