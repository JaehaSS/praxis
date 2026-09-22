/**
 * 100% 누적 막대 + 범례. 색은 Teal 명도 계조만 사용한다 —
 * DESIGN.md "두 번째 유채색 액센트 금지"를 지키면서 계열을 구분하기 위한 선택.
 */
/** 명도를 크게 교대시켜 인접 세그먼트가 붙어 있어도 경계가 보이게 한다. */
const SERIES = [
  { bg: "var(--c-series-1)", fg: "var(--c-bg)" },
  { bg: "var(--c-series-2)", fg: "var(--c-on-accent)" },
  { bg: "var(--c-series-3)", fg: "var(--c-bg)" },
  { bg: "var(--c-series-4)", fg: "var(--c-on-accent)" },
];
const REST = { bg: "var(--c-text-muted)", fg: "var(--c-bg)" };
/** 이 비율 미만이면 막대 안에 라벨을 넣지 않는다(잘림 방지). */
const LABEL_MIN_RATIO = 0.12;

export interface Segment {
  key: string;
  label: string;
  value: number;
}

/** 상위 `top`개만 개별 표시하고 나머지는 "기타"로 합친다. */
function collapse(segments: Segment[], top: number): Segment[] {
  if (segments.length <= top) return segments;
  const head = segments.slice(0, top);
  const rest = segments.slice(top).reduce((sum, s) => sum + s.value, 0);
  return rest > 0 ? [...head, { key: "__rest", label: "기타", value: rest }] : head;
}

export function StackedBar({ segments, top = 4 }: { segments: Segment[]; top?: number }) {
  const items = collapse(
    [...segments].sort((a, b) => b.value - a.value).filter((s) => s.value > 0),
    top,
  );
  const total = items.reduce((sum, s) => sum + s.value, 0);

  if (total === 0) {
    return <div className="text-text-muted text-sm">표시할 데이터가 없습니다</div>;
  }

  const styleOf = (s: Segment, i: number) =>
    s.key === "__rest" ? REST : SERIES[i % SERIES.length];

  return (
    <div>
      <div className="flex h-7 rounded-md overflow-hidden gap-px">
        {items.map((s, i) => {
          const ratio = s.value / total;
          const st = styleOf(s, i);
          return (
            <div
              key={s.key}
              className="flex items-center justify-center text-xs font-medium overflow-hidden"
              style={{ width: `${ratio * 100}%`, background: st.bg, color: st.fg }}
              title={`${s.label} · ${Math.round(ratio * 100)}%`}
            >
              {ratio >= LABEL_MIN_RATIO && `${Math.round(ratio * 100)}%`}
            </div>
          );
        })}
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1 mt-2 text-xs">
        {items.map((s, i) => (
          <span key={s.key} className="flex items-center gap-1.5">
            <span
              className="w-2 h-2 rounded-sm shrink-0"
              style={{ background: styleOf(s, i).bg }}
              aria-hidden
            />
            <span className="text-text-secondary">{s.label}</span>
          </span>
        ))}
      </div>
    </div>
  );
}
