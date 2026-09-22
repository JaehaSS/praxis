import { useMemo } from "react";
import type { Insights, InsightsRange } from "../../../lib/ipc";
import { fmtInt } from "./format";

const DAY_MS = 86400000;
const LEVEL_ALPHA = [0, 0.25, 0.45, 0.7, 1];

/** GitHub식 활동 히트맵 — 로컬 날짜 기준, range에 맞춘 윈도우. */
export function Heatmap({ days, range }: { days: Insights["days"]; range: InsightsRange }) {
  const grid = useMemo(() => {
    const counts = new Map(days.map((d) => [d.date, d.messages]));
    // 백엔드가 로컬 시간대로 버킷팅하므로 today/day 번호도 로컬 기준으로 맞춘다.
    const offMs = -new Date().getTimezoneOffset() * 60000;
    const todayDay = Math.floor((Date.now() + offMs) / DAY_MS);
    // 전체 기간은 실제 데이터 범위에 맞춰 잡는다 — 늘 371일을 그리면 빈 칸만 가득 찬다.
    const firstDay = days.length
      ? Math.floor(Date.parse(`${days[0].date}T00:00:00Z`) / DAY_MS)
      : todayDay;
    const allWin = Math.min(371, Math.max(70, todayDay - firstDay + 8));
    const win = range === "7d" ? 28 : range === "30d" ? 70 : allWin; // 보기 좋은 최소 폭 보장
    const startDay = todayDay - win + 1;
    const dateStr = (day: number) => new Date(day * DAY_MS).toISOString().slice(0, 10);
    const weekday = (day: number) => new Date(day * DAY_MS).getUTCDay(); // 0=일
    const gridStart = startDay - weekday(startDay); // 일요일 정렬
    const cols: { day: number; count: number }[][] = [];
    let max = 0;
    for (let day = gridStart; day <= todayDay; day++) {
      const col = Math.floor((day - gridStart) / 7);
      if (!cols[col]) cols[col] = [];
      const count = day >= startDay ? (counts.get(dateStr(day)) ?? 0) : 0;
      max = Math.max(max, count);
      cols[col][weekday(day)] = { day, count };
    }
    return { cols, max };
  }, [days, range]);

  const level = (c: number) => {
    if (c <= 0) return 0;
    const q = c / Math.max(1, grid.max);
    return q > 0.66 ? 4 : q > 0.33 ? 3 : q > 0.1 ? 2 : 1;
  };

  return (
    <div className="overflow-x-auto pb-1">
      <div className="flex gap-[3px]">
        {grid.cols.map((col, ci) => (
          <div key={ci} className="flex flex-col gap-[3px]">
            {Array.from({ length: 7 }, (_, r) => {
              const cell = col?.[r];
              if (!cell) return <div key={r} className="w-3 h-3" />;
              const lv = level(cell.count);
              return (
                <div
                  key={r}
                  className="w-3 h-3 rounded-[2px] border border-border/40"
                  style={{
                    background:
                      lv === 0
                        ? "var(--c-raised)"
                        : `rgb(from var(--c-primary-bright) r g b / ${LEVEL_ALPHA[lv]})`,
                  }}
                  title={`${new Date(cell.day * DAY_MS).toISOString().slice(0, 10)} · ${fmtInt(cell.count)} 메시지`}
                />
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}
