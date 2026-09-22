// 홈 상태 스트립의 순수 계산 — 홈이 이미 받는 tasks 배열만으로 네 숫자를 만든다.
// 새 IPC를 붙이지 않는 것이 요점이다. 아래 섹션들의 목록과 같은 배열에서 나오므로
// "도는 중 3"과 마을에 선 캐릭터 수가 어긋날 수 없다.

import type { Task } from "../../lib/ipc";
import { isInFlight, needsUser } from "../../lib/task-status";

export interface HomeStats {
  /** 에이전트가 지금 붙어 있는 작업. */
  running: number;
  /** 내가 봐야 진행되는 작업(검토 대기·인증 차단). 실행 승인 대기는 아래 칸이 따로 센다. */
  awaiting: number;
  /** 실행 승인을 기다리는 작업 — 홈 아래 "승인 대기" 섹션과 같은 정의다(ADR 0191). */
  pendingApproval: number;
  /** 로컬 자정 이후 종료된 작업. */
  doneToday: number;
}

/** 로컬 자정(epoch 초). 작업 시각이 초 단위라 같은 단위로 맞춘다. */
function startOfDay(now: Date): number {
  const midnight = new Date(now);
  midnight.setHours(0, 0, 0, 0);
  return Math.floor(midnight.getTime() / 1000);
}

/**
 * 홈 상단 요약. `now`는 테스트가 자정 경계를 고정하려고 주입한다.
 *
 * '오늘 완료'의 기준 시각은 `updated_at`이다 — 종료 상태에서 이 필드는 마지막으로 멈춘
 * 시각을 가리킨다(`lastRunLabel` 참고). 별도의 완료 시각 컬럼은 없다.
 */
export function homeStats(tasks: readonly Task[], now: Date = new Date()): HomeStats {
  const midnight = startOfDay(now);
  const stats: HomeStats = { running: 0, awaiting: 0, pendingApproval: 0, doneToday: 0 };

  for (const task of tasks) {
    if (isInFlight(task)) stats.running += 1;
    if (needsUser(task) && task.state !== "PendingApproval") stats.awaiting += 1;
    if (task.state === "PendingApproval") stats.pendingApproval += 1;
    if (task.state === "Done" && task.updated_at >= midnight) stats.doneToday += 1;
  }

  return stats;
}
