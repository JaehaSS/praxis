// 마지막으로 읽은 주간 회고 — 사이드바 신선도 점의 상태 (설계 0054 DR-6).
//
// DB가 아니라 localStorage인 이유는 이것이 **기기별 UI 상태**이지 공유할 도메인 사실이
// 아니기 때문이다. DB에 두면 마이그레이션이 하나 더 늘고 모바일 PWA와 상태가 엇갈린다.
//
// 켜지는 조건이 "적체 건수"가 아니라 "새 다이제스트"인 것이 요점이다. 만성적으로 켜져
// 있는 배지는 두 주면 무시된다.

const STORAGE_KEY = "praxis:retro-seen";

/** 마지막으로 읽은 주의 `week_start`. 없으면 null. */
export function loadSeenWeek(): number | null {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = Number(raw);
    return Number.isFinite(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

export function saveSeenWeek(weekStart: number): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, String(weekStart));
  } catch {
    // private mode 등 storage 거부 시 현재 세션 상태만 유지한다(praxis:diff-viewed와 같은 방침).
  }
}

/**
 * 읽지 않은 회고가 있는가.
 *
 * 최신 주가 마지막으로 읽은 주보다 **뒤일 때만** 참이다. 같거나 앞이면 거짓 —
 * 지난 주를 다시 열어봤다고 점이 다시 켜지면 안 된다.
 */
export function hasUnread(latestWeek: number | null, seenWeek: number | null): boolean {
  if (latestWeek == null) return false;
  return seenWeek == null || latestWeek > seenWeek;
}
