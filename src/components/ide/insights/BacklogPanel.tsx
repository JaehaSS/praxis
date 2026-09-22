import { useEffect, useState, type ReactElement } from "react";
import { todayList } from "../../../lib/ipc";
import { BACKLOG, type DayItem } from "../today-items";
import { backlogStats, ROT_DAYS, STALE_DAYS } from "./backlog-stats";
import { Metric } from "./parts";

export interface BacklogPanelApi {
  list: (day?: string) => Promise<DayItem[]>;
}

interface Props {
  /** Home으로 보내는 링크. 없으면 링크를 숨긴다. */
  onOpenHome?: () => void;
  /** 테스트에서 "지금"을 고정하기 위한 주입점. */
  nowMs?: number;
  api?: BacklogPanelApi;
}

/**
 * 백로그 적체 — **읽기 전용**이다.
 *
 * 여기서 오늘로 당기거나 버리지 않는다. 인사이트는 지나간 것의 집계이고 편집은 홈의 몫이라는
 * 그 탭의 성격을 지킨다 (플랜 0054 Phase 3). 이 패널이 하는 일은 하나뿐이다 —
 * 백로그가 무덤이 되고 있는지 알려 주는 것.
 */
export function BacklogPanel({ onOpenHome, nowMs, api }: Props = {}): ReactElement | null {
  const [items, setItems] = useState<DayItem[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    const list = api?.list ?? todayList;
    list(BACKLOG)
      // 실패는 조용히 빈 목록으로 — 계획 탭의 캘린더까지 죽이지 않는다.
      .catch(() => [] as DayItem[])
      .then((rows) => {
        if (!cancelled) setItems(rows);
      });
    return () => {
      cancelled = true;
    };
  }, [api]);

  if (items === null) return null;

  const nowSecs = Math.floor((nowMs ?? Date.now()) / 1000);
  const stats = backlogStats(items, nowSecs);

  if (stats.total === 0) {
    return (
      <div className="mt-6 text-sm text-text-muted">백로그에 쌓인 것이 없습니다.</div>
    );
  }

  return (
    <div className="mt-6">
      <div className="mb-3 flex items-baseline justify-between">
        <h3 className="text-sm font-medium text-text-secondary">백로그 적체</h3>
        {onOpenHome && (
          <button
            type="button"
            className="text-xs text-text-muted hover:text-text"
            onClick={onOpenHome}
          >
            홈에서 정리하기
          </button>
        )}
      </div>

      <div className="grid grid-cols-3 gap-2">
        <Metric label="쌓인 항목" value={`${stats.total}`} />
        <Metric label={`${STALE_DAYS}일 이상`} value={`${stats.stale30}`} />
        {/* 90일을 넘긴 것은 "언젠가"가 아니라 "안 할 일"이라는 신호다. 그래서 강조한다. */}
        <Metric
          label={`${ROT_DAYS}일 이상`}
          value={`${stats.stale90}`}
          accent={stats.stale90 > 0}
        />
      </div>

      <div className="mt-3 overflow-hidden rounded-md border border-border">
        {stats.oldest.map((entry) => (
          <div
            key={entry.id}
            className="flex items-center gap-2 border-b border-border px-3 py-2 last:border-b-0"
          >
            <span className="flex-1 truncate text-sm text-text-secondary" title={entry.title}>
              {entry.title}
            </span>
            <span className="shrink-0 font-code text-xs text-text-muted">{entry.days}일</span>
          </div>
        ))}
      </div>
    </div>
  );
}
