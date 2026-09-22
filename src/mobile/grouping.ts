import type { Task } from "../lib/ipc";
import { actionableCount, sortForMobile } from "./status";

// 작업 목록을 프로젝트 섹션으로 나눈다 — 순수 로직. (설계 0013 §5.3)
//
// 데스크톱 사이드바는 이미 레포별 그룹 + 진행/지난 작업 분리를 쓴다. 모바일도 같은 모델을
// 따르되, 폰은 한 화면에 몇 줄 못 보므로 **섹션 순서 자체가 정보 설계**가 된다.
// 내 결정을 기다리는 작업이 있는 프로젝트를 위로 올린다.

/** 레포 경로에서 마지막 세그먼트. 좁은 화면에서 전체 경로는 읽히지 않는다. */
export function repoName(repo: string): string {
  const parts = repo.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? repo;
}

/** 아직 끝나지 않은 작업. 끝난 것과 섞으면 섹션이 이력으로 가득 찬다. */
export function isActive(state: string): boolean {
  return !(state === "Done" || state === "Discarded" || state === "Failed");
}

export interface RepoGroup {
  /** 전체 경로 — 접힘 상태 키이자 React key. 이름만 쓰면 동명 프로젝트가 합쳐진다. */
  repo: string;
  name: string;
  tasks: Task[];
  /** 내 결정을 기다리는 작업 수 — 섹션 헤더 배지. */
  actionable: number;
  /** 이 프로젝트의 마지막 활동 시각. */
  updatedAt: number;
}

/**
 * 진행 중 작업을 프로젝트별로 묶는다.
 *
 * 섹션 순서: 행동이 필요한 프로젝트 → 최근 활동 순.
 * 개수가 아니라 유무로 먼저 가르는 이유는, 오래된 검토 대기 5건이 방금 온 1건을 계속
 * 밀어내지 않게 하기 위해서다.
 */
export function groupByRepo(tasks: Task[]): RepoGroup[] {
  const buckets = new Map<string, Task[]>();
  for (const task of tasks) {
    if (!isActive(task.state)) continue;
    const bucket = buckets.get(task.repo);
    if (bucket) bucket.push(task);
    else buckets.set(task.repo, [task]);
  }
  const groups: RepoGroup[] = [];
  for (const [repo, bucket] of buckets) {
    groups.push({
      repo,
      name: repoName(repo),
      tasks: sortForMobile(bucket),
      actionable: actionableCount(bucket),
      updatedAt: bucket.reduce((latest, task) => Math.max(latest, task.updated_at), 0),
    });
  }
  return groups.sort((a, b) => {
    const byAction = Number(b.actionable > 0) - Number(a.actionable > 0);
    if (byAction !== 0) return byAction;
    if (b.updatedAt !== a.updatedAt) return b.updatedAt - a.updatedAt;
    // 활동 시각까지 같으면 이름순 — 새로고침마다 순서가 흔들리면 눈이 길을 잃는다.
    return a.name.localeCompare(b.name, "en", { sensitivity: "base" });
  });
}

/** 지난 작업 목록에 보여줄 최대 건수. 폰에서 이력을 끝없이 스크롤할 일은 없다. */
export const FINISHED_LIMIT = 20;

/** 끝난 작업은 프로젝트와 무관하게 한 곳에 모아 최근 것만 보여준다. */
export function finishedTasks(tasks: Task[], limit = FINISHED_LIMIT): Task[] {
  return tasks
    .filter((task) => !isActive(task.state))
    .sort((a, b) => b.updated_at - a.updated_at)
    .slice(0, limit);
}
