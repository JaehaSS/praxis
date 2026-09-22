// 홈 '최근' 목록의 순수 로직 — 단일 작업과 앙상블 그룹을 한 줄기로 합친다.
// 비교 실행이 별도 섹션이던 시절에는 두 목록이 각자 최신순이라 홈에서 시간이 두 번 흘렀다.

import type { Task } from "../../lib/ipc";

/** 후보가 심판 대상이 된 상태 — 자율수행이 끝나 사람이 볼 수 있다. */
const READY_STATES = ["AwaitingReview", "Done"];
const HIDDEN_STATES = new Set(["Done", "Discarded"]);

interface Common {
  /** React key. 작업과 앙상블은 id 공간이 달라 접두사로 가른다. */
  key: string;
  /** 정렬 기준 시각. 앙상블은 그룹에서 가장 최근 후보를 대표로 삼는다. */
  at: number;
}

export interface RecentTask extends Common {
  kind: "task";
  task: Task;
}

export interface RecentEnsemble extends Common {
  kind: "ensemble";
  id: string;
  /** 생성 순 후보들. 배지 순서가 렌더마다 흔들리지 않게 정렬해 둔다. */
  tasks: Task[];
  /** 자율수행이 끝난 후보 수 — `ready/tasks.length`로 진행을 보여준다. */
  ready: number;
}

export type RecentItem = RecentTask | RecentEnsemble;

/** 목록에서 이 항목을 대표하는 작업 id. 시각이 같을 때의 결정적 tiebreak. */
const rank = (item: RecentItem): number =>
  item.kind === "task" ? item.task.id : Math.max(...item.tasks.map((t) => t.id));

/**
 * 최근 목록. 앙상블 후보는 개별 행으로 흩어지지 않고 그룹 한 행으로 접힌다 —
 * 3벤더 비교 한 번이 최근을 세 줄 먹으면 나머지 작업이 화면 밖으로 밀린다.
 */
export function recentItems(tasks: Task[], limit = 6): RecentItem[] {
  const groups = new Map<string, Task[]>();
  const items: RecentItem[] = [];

  for (const task of tasks) {
    if (task.ensemble) {
      groups.set(task.ensemble, [...(groups.get(task.ensemble) ?? []), task]);
    } else if (!HIDDEN_STATES.has(task.state)) {
      items.push({ kind: "task", key: `task:${task.id}`, at: task.created_at, task });
    }
  }

  for (const [id, members] of groups) {
    if (members.every((task) => HIDDEN_STATES.has(task.state))) continue;
    const ordered = [...members].sort((a, b) => a.created_at - b.created_at || a.id - b.id);
    items.push({
      kind: "ensemble",
      key: `ensemble:${id}`,
      at: ordered.reduce((max, t) => Math.max(max, t.created_at), 0),
      id,
      tasks: ordered,
      ready: ordered.filter((t) => READY_STATES.includes(t.state)).length,
    });
  }

  return items.sort((a, b) => b.at - a.at || rank(b) - rank(a)).slice(0, limit);
}
