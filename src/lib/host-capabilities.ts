import { LOCAL_HOST, type HostId } from "./transport";

/**
 * 호스트가 무엇을 할 수 있는가. **누르기 전에** 판정하기 위한 표다 (ADR 0133 결정 6).
 *
 * transport 쪽 `Promise.reject`는 그대로 남는다 — UI 가드가 바뀌어도 호출이 조용히
 * 성공하지 않게 하는 두 번째 방어선이다. 여기는 첫 번째 방어선이고, 목적이 다르다:
 * 사용자가 눌러 보고 나서야 안 되는 걸 알게 되지 않도록.
 */
export interface HostCapabilities {
  /** 파일 생성·이름변경·복제·휴지통, 외부 앱으로 열기. Runner에 대응 엔드포인트가 없다. */
  fileOperations: boolean;
  /** 인터뷰·그릴 — Runner에 헤드리스 CLI 위임 경로가 없다. */
  interview: boolean;
  /** 정의·구현·사용처 이동. 언어 서버가 워크트리 파일시스템을 직접 읽는다. */
  lsp: boolean;
  /** Design Mode 프리뷰 — 자식 웹뷰가 로컬 프로세스다. */
  designPreview: boolean;
  /** base 브랜치 선택 — 원격은 러너 머신의 현재 체크아웃에서 분기한다. */
  baseBranch: boolean;
  /** 워크스페이스 셸(PTY) — 로컬 프로세스다. */
  workspaceShell: boolean;
  /** 세션 에이전트 전환·토론 시작. 모델 교체(`taskModelSet`)는 여기 없다 — 그쪽은 양쪽 다 된다.
   *
   *  전환은 벤더 세션을 버리고 핸드오프를 재조립하는 경로라 Runner에 대응 엔드포인트가 없고,
   *  토론은 Runner의 대화 어댑터가 우측 자리가 있는 작업을 아예 거절한다. */
  sessionAgentSwitch: boolean;
  /** 끝난 대화를 새 작업으로 이어받기(`task_resume`) — resume 체인이 로컬 DB 전용이라 Runner엔 없다. */
  convoResume: boolean;
  /** 세션홈에서 벤더 세션을 골라 새 작업으로 이어받기(`resume_session`, 설계 2026-09-17).
   *  `convoResume`과 다른 플래그다 — 그쪽은 로컬 DB의 resume 체인 전용이라 원격에서 항상
   *  false지만, 이 경로는 `taskCreate`를 그대로 타서 원격도 지원한다(결정 2). */
  sessionHomeResume: boolean;
}

const LOCAL: HostCapabilities = {
  fileOperations: true,
  interview: true,
  lsp: true,
  designPreview: true,
  baseBranch: true,
  workspaceShell: true,
  sessionAgentSwitch: true,
  convoResume: true,
  sessionHomeResume: true,
};

const REMOTE: HostCapabilities = {
  fileOperations: false,
  interview: false,
  lsp: false,
  designPreview: false,
  baseBranch: false,
  workspaceShell: false,
  sessionAgentSwitch: false,
  convoResume: false,
  sessionHomeResume: true,
};

/** 비활성 손잡이에 붙이는 사유. 감추기보다 비활성화가 기본이다 — 사라지면 "원래 없는 기능"으로 읽힌다. */
export const LOCAL_ONLY_REASON = "로컬 세션에서만 쓸 수 있습니다";

export function hostCapabilities(host: HostId): HostCapabilities {
  return host === LOCAL_HOST ? LOCAL : REMOTE;
}
