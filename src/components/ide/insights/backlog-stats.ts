// 백로그 적체 집계 — 렌더와 IPC에서 분리해 단위 테스트한다 (플랜 0054 Phase 3).
//
// 백로그의 알려진 실패 모드는 "적기만 하고 아무도 안 보는 무덤"이다. 이 집계가 그것을
// 감지하는 유일한 장치이므로, 숫자만 세지 않고 **무엇이** 썩고 있는지 이름까지 낸다.
// 총량만 보여 주면 아무도 움직이지 않는다.

import { staleDays, type DayItem } from "../today-items";

/** 이 날수를 넘기면 적체로 본다. `BacklogSection`의 STALE_DAYS와 같은 기준. */
export const STALE_DAYS = 30;
/** 이 날수를 넘기면 "이 항목은 사실 하지 않을 일"이라는 신호로 본다. */
export const ROT_DAYS = 90;
/** 이름을 내는 항목 수. 더 늘리면 인사이트가 백로그 목록의 사본이 된다. */
const NAMED = 5;

export interface BacklogStats {
  total: number;
  /** 30일 이상 묵은 건수 */
  stale30: number;
  /** 90일 이상 묵은 건수 */
  stale90: number;
  /** 가장 오래된 항목의 적체 일수. 비었으면 0. */
  oldestDays: number;
  /** 오래된 순 상위 몇 건 — 숫자가 아니라 이름이 행동을 부른다. */
  oldest: { id: number; title: string; days: number }[];
}

export function backlogStats(items: DayItem[], nowSecs: number): BacklogStats {
  const aged = items
    .map((i) => ({ id: i.id, title: i.title, days: staleDays(i, nowSecs) }))
    // 같은 날 담은 것들 사이에서 순서가 렌더마다 흔들리지 않게 id로 tiebreak.
    .sort((a, b) => b.days - a.days || a.id - b.id);

  return {
    total: aged.length,
    stale30: aged.filter((i) => i.days >= STALE_DAYS).length,
    stale90: aged.filter((i) => i.days >= ROT_DAYS).length,
    oldestDays: aged[0]?.days ?? 0,
    oldest: aged.slice(0, NAMED),
  };
}
