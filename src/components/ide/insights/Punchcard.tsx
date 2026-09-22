import { fmtHour, fmtInt } from "./format";

const WEEKDAYS = ["일", "월", "화", "수", "목", "금", "토"];
/** 열 레이블을 다는 시각 — 3시간 간격. */
const HOUR_TICKS = [0, 3, 6, 9, 12, 15, 18, 21];
const LEVEL_ALPHA = [0, 0.25, 0.45, 0.7, 1];

/** 최대값 대비 비율 → 0~4 강도. */
function level(count: number, max: number) {
  if (count <= 0) return 0;
  const q = count / Math.max(1, max);
  return q > 0.66 ? 4 : q > 0.33 ? 3 : q > 0.1 ? 2 : 1;
}

/**
 * 요일 × 시간 펀치카드 — `weekdayHours`는 168칸(인덱스 = 요일*24 + 시).
 * 24칸 막대보다 정보량이 많아 작업 리듬(평일 낮 / 주말 밤 등)이 한눈에 드러난다.
 */
export function Punchcard({ weekdayHours }: { weekdayHours: number[] }) {
  if (weekdayHours.length < 168) {
    return <div className="text-text-muted text-sm">표시할 데이터가 없습니다</div>;
  }
  const max = Math.max(0, ...weekdayHours);

  return (
    <div className="overflow-x-auto">
      {/* 넓은 화면에서 셀이 가로로 늘어지지 않도록 격자 폭을 묶는다(셀이 대략 정사각). */}
      <div className="min-w-[420px] max-w-[640px]">
        {WEEKDAYS.map((label, w) => (
          <div key={w} className="flex items-center gap-1.5 mb-[3px]">
            <span className="w-4 text-xs text-text-muted shrink-0">{label}</span>
            <div
              className="grid gap-[3px] flex-1"
              style={{ gridTemplateColumns: "repeat(24, minmax(0, 1fr))" }}
            >
              {Array.from({ length: 24 }, (_, h) => {
                const count = weekdayHours[w * 24 + h] ?? 0;
                const lv = level(count, max);
                return (
                  <div
                    key={h}
                    className="h-3.5 rounded-[2px] border border-border/40"
                    style={{
                      background:
                        lv === 0
                          ? "var(--c-raised)"
                          : `rgb(from var(--c-primary-bright) r g b / ${LEVEL_ALPHA[lv]})`,
                    }}
                    title={`${label}요일 ${fmtHour(h)} · ${fmtInt(count)} 메시지`}
                  />
                );
              })}
            </div>
          </div>
        ))}
        <div className="flex items-center gap-1.5">
          <span className="w-4 shrink-0" />
          <div
            className="grid flex-1 gap-[3px] text-xs text-text-muted"
            style={{ gridTemplateColumns: "repeat(24, minmax(0, 1fr))" }}
          >
            {Array.from({ length: 24 }, (_, h) => (
              <span key={h} className="text-center leading-4">
                {HOUR_TICKS.includes(h) ? h : ""}
              </span>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
