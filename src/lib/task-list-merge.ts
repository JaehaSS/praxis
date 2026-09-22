import type { Task } from "./ipc";
import type { HostId } from "./transport";

/** 호스트 하나의 조회 결과. `tasks`가 null이면 그 호스트만 실패한 것이다. */
export interface HostTaskResult {
  host: HostId;
  tasks: Task[] | null;
  error: string | null;
  /**
   * 연결을 기대하지 않는 호스트 — 저장만 돼 있고 붙지 않은 프로필. 캐시 행은 합치되
   * 실패로 알리지 않는다. 안 붙은 것은 오류가 아니라 쉬는 상태다.
   */
  dormant?: boolean;
}

export interface HostFailure {
  host: HostId;
  error: string;
}

export interface MergedTaskList {
  tasks: Task[];
  /** 응답하지 않은 호스트. 목록은 나머지 호스트 것만으로 그린다. */
  failures: HostFailure[];
}

/**
 * 호스트별 결과를 한 목록으로 합친다.
 *
 * **부분 실패를 허용한다** — 호스트 하나가 죽었다고 나머지 호스트의 작업까지 감추면
 * 원격이 끊긴 순간 로컬 작업도 사라진다. 실패는 목록에서 빼는 대신 따로 알린다.
 *
 * 정렬은 호스트 무관 `updated_at` 내림차순이다. 호스트별로 묶으면 시간순이 깨져
 * "방금 만든 것"을 목록에서 찾을 수 없게 된다.
 */
export function mergeTaskLists(
  results: HostTaskResult[],
  cachedTasks: (host: HostId) => Task[] = () => [],
): MergedTaskList {
  const tasks: Task[] = [];
  const failures: HostFailure[] = [];
  for (const result of results) {
    if (result.tasks === null) {
      if (!result.dormant) failures.push({ host: result.host, error: result.error ?? "응답 없음" });
      tasks.push(...cachedTasks(result.host));
      continue;
    }
    tasks.push(...result.tasks);
  }
  // 같은 id라도 호스트가 다르면 서로 다른 작업이다 — 여기서 합치거나 걸러내지 않는다.
  tasks.sort((left, right) => right.updated_at - left.updated_at);
  return { tasks, failures };
}
