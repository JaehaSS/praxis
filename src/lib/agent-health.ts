//! 에이전트 CLI 상태의 표시 규칙 — 컴포넌트에서 분리해 xterm 없이 테스트한다.
//! (`lib/usage.ts`가 `UsageBar`에 대해 갖는 관계와 같다.)

import type {
  AuthState,
  HubUpdate,
  InstallMethod,
  UpdateOutcome,
  VendorHealth,
} from "./ipc";

/** 인증 상태 → 표시. `unknown`은 실패가 아니라 "확인 못 함"이다 — 빨강으로 겁주지 않는다. */
export function authBadge(auth: AuthState): { mark: string; text: string; color: string } {
  switch (auth) {
    case "ok":
      return { mark: "✓", text: "로그인됨", color: "text-status-done" };
    case "logged_out":
      return { mark: "!", text: "로그인 필요", color: "text-status-failed" };
    default:
      return { mark: "?", text: "확인 불가", color: "text-text-muted" };
  }
}

/** 버전 한 줄. 업데이트가 있으면 화살표로, 없으면 최신임을 명시한다. */
export function versionLabel(health: VendorHealth): string {
  if (!health.installed) return "설치 확인 불가";
  if (health.update_available && health.latest) {
    return `${health.installed} → ${health.latest}`;
  }
  if (health.latest && health.installed === health.latest) {
    return `${health.installed} · 최신`;
  }
  return health.installed;
}

const METHOD_LABEL: Record<InstallMethod, string> = {
  native: "native 설치",
  npm: "npm 전역",
  homebrew: "Homebrew",
  unknown: "설치 방식 미상",
};

export function methodLabel(method: InstallMethod): string {
  return METHOD_LABEL[method] ?? METHOD_LABEL.unknown;
}

/** 계정·플랜·부연을 한 줄로. 없는 것은 조용히 빠진다. */
export function accountLine(health: VendorHealth): string {
  const parts = [health.account, health.plan, health.auth_detail].filter(
    (part): part is string => typeof part === "string" && part.length > 0,
  );
  return parts.join(" · ");
}

/** 업데이트 버튼을 누를 수 있는가. 설치 방식을 모르면 명령을 만들 수 없다. */
export function canUpdate(health: VendorHealth): boolean {
  return health.update_available && health.install_method !== "unknown";
}

/** 업데이트 버튼 비활성 사유 — 회색 버튼만 있으면 왜 못 누르는지 알 수 없다. */
export function updateHint(health: VendorHealth): string | null {
  if (!health.update_available) return null;
  if (health.install_method === "unknown") {
    return "설치 방식을 알 수 없어 업데이트 명령을 만들 수 없습니다";
  }
  return null;
}

/** 자동 업데이트 결과 한 줄.
 *
 *  명령 성공과 버전 변화를 구분한다 — `claude update`는 이미 최신일 때도 성공으로 끝나므로
 *  "성공"만 보여주면 무엇이 달라졌는지 알 수 없다. */
export function outcomeLine(outcome: UpdateOutcome): string {
  const { label, from, to, ok, error } = outcome;
  if (!ok) return `${label} · 실패${error ? `: ${error}` : ""}`;
  // 명령은 성공했는데 버전을 못 읽었다. "변화 없음"이라 말하면 모르는 것을 안다고 하는 셈이다.
  if (!to) return `${label} · 업데이트함 · 버전 확인 실패`;
  if (from === to) return `${label} · 변화 없음`;
  // 이전 버전을 몰랐던 경우(설치가 없다가 새로 깔림)도 변화는 변화다.
  if (!from) return `${label} → ${to}`;
  return `${label} ${from} → ${to}`;
}

/** Antigravity Hub에 대해 사용자에게 할 말. 할 말이 없으면 null이다.
 *
 *  최신일 때는 행을 그리지 않는다 — 이 표시의 목적은 "밀린 업데이트를 알리는 것"이고,
 *  아무 일도 없을 때 자리를 차지하면 잡음이 된다. */
export function hubNotice(hub: HubUpdate): string | null {
  if (!hub.restart_required) return null;
  const from = hub.installed ?? "현재";
  const to = hub.downloaded ?? "새 버전";
  return `Antigravity ${from} → ${to} · 앱을 재시작하면 적용됩니다`;
}
