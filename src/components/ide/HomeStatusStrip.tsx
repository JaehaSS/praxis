import { useMemo } from "react";
import type { Task } from "../../lib/ipc";
import { Icon, type IconName } from "./icons";
import { homeStats } from "./home-stats";

interface CellProps {
  icon: IconName;
  label: string;
  value: string;
  /** 값이 있을 때의 강조색. 0이거나 값이 없으면 넘기지 않아 중립으로 남는다. */
  color?: string;
}

/**
 * 스트립 한 칸. 색은 보조 채널이고 아이콘·라벨이 항상 함께 선다 —
 * 색만으로 상태를 전달하지 않는다(DESIGN.md).
 */
function Cell({ icon, label, value, color }: CellProps) {
  return (
    <div className="flex items-center gap-2 px-3 py-2.5 min-w-0 border-border border-b last:border-b-0 md:border-b-0 md:border-r md:last:border-r-0">
      <span style={color ? { color } : undefined} className={color ? undefined : "text-text-muted"}>
        <Icon name={icon} size={14} />
      </span>
      <span className="text-xs text-text-secondary truncate">{label}</span>
      <span
        data-cell-value=""
        className="ml-auto shrink-0 text-sm font-code"
        style={color ? { color } : undefined}
      >
        {value}
      </span>
    </div>
  );
}

interface Props {
  tasks: Task[];
}

/**
 * 홈 상단 한 줄 요약. 넓은 화면에서 남는 가로를 가장 싸게 쓰는 자리다.
 *
 * 숫자만 세우고 목록은 아래 섹션들이 그대로 맡는다 — 홈을 구경하는 대시보드로
 * 만들지 않는 것이 이 컴포넌트의 경계다.
 *
 * 벤더 잔량은 여기 서지 않는다 — 하단 UsageBar가 같은 값을 늘 보여주므로 폴러도
 * 하나면 된다(ADR 0191). 그래서 이 컴포넌트는 `tasks` 말고 아무것도 읽지 않는다.
 */
export function HomeStatusStrip({ tasks }: Props) {
  const { running, awaiting, pendingApproval, doneToday } = useMemo(() => homeStats(tasks), [tasks]);

  return (
    <section
      className="grid grid-cols-1 md:grid-cols-4 border border-border rounded-md bg-surface mb-6"
      aria-label="현황 요약"
    >
      <Cell
        icon="play"
        label="도는 중"
        value={String(running)}
        color={running > 0 ? "var(--c-running)" : undefined}
      />
      <Cell
        icon="eye"
        label="검토 대기"
        value={String(awaiting)}
        color={awaiting > 0 ? "var(--c-awaiting)" : undefined}
      />
      {/* 검토 대기와 가르는 이유는 행동이 달라서다 — 이쪽은 실행을 허락해야 움직인다. */}
      <Cell
        icon="lock"
        label="승인 대기"
        value={String(pendingApproval)}
        color={pendingApproval > 0 ? "var(--c-awaiting)" : undefined}
      />
      <Cell
        icon="check"
        label="오늘 완료"
        value={String(doneToday)}
        color={doneToday > 0 ? "var(--c-done)" : undefined}
      />
    </section>
  );
}
