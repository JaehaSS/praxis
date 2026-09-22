// 오늘 할 일 순수 로직 — 렌더와 IPC에서 분리해 단위 테스트한다.
// DayItem 타입은 Rust `today::DayItem`의 거울이다 (src-tauri/src/today/mod.rs).

export type DayItemStatus = "open" | "done" | "dropped";

export interface DayItem {
  id: number;
  day: string;
  title: string;
  note: string | null;
  status: DayItemStatus;
  position: number;
  repo: string | null;
  task_id: number | null;
  /** `carry`는 더 이상 생기지 않는다 — 이월이 제안이던 시절에 담은 과거 행에만 남아 있다. */
  source: "manual" | "carry" | "awaiting" | "github" | "memory";
  source_ref: string | null;
  created_at: number;
  updated_at: number;
  done_at: number | null;
  /** 이월돼 온 항목이면 직전에 있던 날('YYYY-MM-DD'). 최초 계획일이 아니다. */
  carried_from: string | null;
}

/** Rust `today::suggest::Suggestion`의 거울. */
export interface DaySuggestion {
  title: string;
  source: string;
  source_ref: string | null;
  repo: string | null;
}

export interface Progress {
  done: number;
  total: number;
  ratio: number;
}

/**
 * 진행률. `dropped`(안 하기로 했다)는 분모에서 뺀다 — 접은 계획이 달성률을 깎으면
 * 사용자가 항목을 접지 않고 방치하게 되고, 그러면 목록이 죽은 항목으로 찬다.
 */
export function progress(items: DayItem[]): Progress {
  const counted = items.filter((i) => i.status !== "dropped");
  const done = counted.filter((i) => i.status === "done").length;
  const total = counted.length;
  return { done, total, ratio: total === 0 ? 0 : done / total };
}

/** position 오름차순, 동률이면 id 오름차순. 원본을 변형하지 않는다. */
export function sortItems(items: DayItem[]): DayItem[] {
  return [...items].sort((a, b) => a.position - b.position || a.id - b.id);
}

/** 항목을 delta칸 옮긴 뒤의 id 순서. 경계를 넘으면 원래 순서를 그대로 돌려준다. */
export function moveItem(items: DayItem[], id: number, delta: number): number[] {
  const ordered = sortItems(items);
  const from = ordered.findIndex((i) => i.id === id);
  const ids = ordered.map((i) => i.id);
  if (from < 0) return ids;
  const to = from + delta;
  if (to < 0 || to >= ordered.length) return ids;
  const [moved] = ids.splice(from, 1);
  ids.splice(to, 0, moved);
  return ids;
}

/** 경로의 마지막 세그먼트 — 배지에 절대 경로를 다 적을 수는 없다. */
export function repoBadge(repo: string): string {
  return repo.split("/").filter(Boolean).pop() ?? repo;
}

/**
 * 항목에 레포 배지를 붙일지. 목록이 한 레포에만 걸쳐 있고 그게 지금 보고 있는 레포라면
 * 모든 줄에 같은 이름이 반복될 뿐이라 붙이지 않는다. 레포가 섞였거나 다른 레포의 일이
 * 끼어 있을 때만 "어느 레포 일인지"가 정보가 된다.
 */
export function showRepoBadges(items: DayItem[], current: string | undefined): boolean {
  const repos = new Set(items.map((i) => i.repo).filter((r): r is string => !!r));
  if (repos.size === 0) return false;
  if (repos.size > 1) return true;
  return [...repos][0] !== current;
}

/**
 * 이월돼 온 항목의 출처 표시. 이월이 아니면 `null`.
 *
 * "며칠째 밀렸다"를 세지 않는다 — `carried_from`은 직전 날짜라 애초에 셀 수 없고,
 * 밀린 항목을 정리할지는 사용자가 `dropped`로 직접 판단한다 (설계 0021 Changelog).
 * 여기서 하는 일은 "이건 오늘 정한 게 아니다"를 조용히 알려 주는 것뿐이다.
 */
export function carriedLabel(item: DayItem): string | null {
  if (!item.carried_from) return null;
  const previous = new Date(`${item.day}T00:00:00Z`);
  previous.setUTCDate(previous.getUTCDate() - 1);
  if (item.carried_from === previous.toISOString().slice(0, 10)) return "어제";
  const [, month, dayOfMonth] = item.carried_from.split("-");
  return `${Number(month)}/${Number(dayOfMonth)}`;
}

/** 제안을 출처별로 묶는다. 화면에서 "왜 이게 떴는지"를 한눈에 보여주기 위한 그룹핑. */
export function groupSuggestions(
  suggestions: DaySuggestion[],
): { source: string; label: string; items: DaySuggestion[] }[] {
  // 지난 날의 미완료는 제안에 없다 — 목록으로 직접 이월된다 (src-tauri/src/today/carry.rs).
  const order = ["awaiting", "github", "memory"];
  const labels: Record<string, string> = {
    awaiting: "검토 대기가 오래된 작업",
    github: "열린 이슈",
    memory: "원장 미해결",
  };
  const grouped = new Map<string, DaySuggestion[]>();
  for (const s of suggestions) {
    grouped.set(s.source, [...(grouped.get(s.source) ?? []), s]);
  }
  return [...grouped.entries()]
    .sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]))
    .map(([source, items]) => ({ source, label: labels[source] ?? source, items }));
}

/**
 * 백로그 레인의 `day` 값 — Rust `today::day::BACKLOG`의 거울.
 *
 * 날짜가 아니다. "할 건데 오늘은 아니다"를 담는 자리이고, 어떤 날짜의 목록·진행률·마감
 * 집계·계획 캘린더에도 나타나지 않는다 (플랜 0054).
 */
export const BACKLOG = "backlog";

/**
 * 항목이 만들어진 뒤 지난 날수. 백로그의 적체를 재는 유일한 눈금이다.
 *
 * `carried_from`(직전에 있던 날)으로는 잴 수 없다 — 백로그로 밀 때 그 값이 지워지고,
 * 애초에 "며칠째 밀렸다"가 아니라 "언제 적어 둔 것이냐"가 적체의 정의다.
 */
export function staleDays(item: Pick<DayItem, "created_at">, nowSecs: number): number {
  return Math.max(0, Math.floor((nowSecs - item.created_at) / 86400));
}
