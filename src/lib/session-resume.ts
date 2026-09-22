// src/lib/session-resume.ts — 세션홈 이어받기 화면의 순수 로직(경고 게이트·오류 번역).
// UI/transport 비의존(테스트 용이성) — quickopen.ts와 같은 자리.

import type { SessionHomeSession, SessionResumeError } from "./transport";

/**
 * "최근 활동" 경고 임계값(초). 벤더 세션 이어받기는 **같은 세션 파일에 이어 쓴다** — 다른
 * 터미널에서 그 세션이 아직 열려 있으면 두 대화가 한 파일에 뒤섞인다(데이터 손상). 너무
 * 짧으면 방금 끝낸 정상 세션까지 매번 확인을 요구하고, 너무 길면 몇 시간 전 세션도 계속
 * 경고해 경고 자체가 무뎌진다. 사람이 터미널 사이를 오가며 딴짓하다 돌아오는 전형적인
 * 텀을 기준으로 5분을 잡는다 — 그 안이면 다른 창에서 여전히 타이핑 중일 가능성이 실재한다.
 */
export const RECENTLY_ACTIVE_THRESHOLD_SECONDS = 5 * 60;

/** `last_active`(unix 초)가 임계값 안이면 다른 터미널에서 여전히 쓰고 있을 수 있다. */
export function isRecentlyActive(
  lastActiveSecs: number,
  nowSecs: number = Math.floor(Date.now() / 1000),
): boolean {
  return nowSecs - lastActiveSecs < RECENTLY_ACTIVE_THRESHOLD_SECONDS;
}

/** 경로 접두 비교. trailing slash 유무로 오탐하지 않도록 정규화한다. */
function normalizePath(path: string): string {
  return path.replace(/\/+$/, "");
}

/**
 * 세션의 작업 디렉터리(cwd, 없으면 last_cwd)가 선택된 저장소 밖이면 true. `all=true`로
 * 저장소 접두 필터를 걷고 고른 세션일 때만 실제로 걸린다 — 기본 필터(설계 결정 10)를 쓰면
 * 애초에 이 조건을 만족하는 세션은 목록에 뜨지 않는다.
 */
export function isDifferentRepository(session: SessionHomeSession, repo: string): boolean {
  const cwd = session.cwd ?? session.last_cwd;
  if (!cwd) return true; // cwd를 아예 모르면 같은 저장소라고 보장할 수 없다 — 경고 쪽으로 기운다.
  const normalizedRepo = normalizePath(repo);
  const normalizedCwd = normalizePath(cwd);
  return normalizedCwd !== normalizedRepo && !normalizedCwd.startsWith(`${normalizedRepo}/`);
}

export interface SessionResumeErrorDescription {
  message: string;
  /** 409(충돌)일 때만 채워진다 — UI가 그 작업으로 이동하는 링크를 보여줄 수 있게. */
  taskId?: number;
}

/**
 * 세션 승계 거절을 사람이 읽는 문구로 번역한다. `not_found`는 "없음"과 "인가 밖"을 **의도적으로**
 * 뭉친다 — 어느 쪽인지 구분해 보여주면 존재를 열거하는 통로가 된다(설계 2026-09-17 결정 9).
 */
export function describeSessionResumeError(error: SessionResumeError): SessionResumeErrorDescription {
  if (error.kind === "conflict") {
    return {
      message:
        error.taskId != null
          ? `이미 #${error.taskId} 작업이 이어가고 있습니다`
          : "이미 다른 작업이 이어가고 있습니다",
      taskId: error.taskId,
    };
  }
  return { message: "찾을 수 없거나 접근 권한이 없습니다" };
}
