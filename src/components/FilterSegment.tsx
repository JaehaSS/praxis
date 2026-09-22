import { useRef, type KeyboardEvent, type ReactElement } from "react";

export interface FilterSegmentItem {
  /** 세그먼트 식별자 — onChange로 그대로 돌아온다. */
  value: string;
  label: string;
  /** 라벨 뒤에 병기할 개수. 0이면 렌더하지 않는다(alwaysVisible·선택된 값은 예외). */
  count: number;
  /** 라벨만으로 범위가 분명하지 않을 때의 설명 — `활성`이 무엇을 빼는지 같은 것. */
  title?: string;
}

interface Props {
  /** tablist 접근성 이름 — 무엇의 범위를 좁히는지. */
  label: string;
  items: FilterSegmentItem[];
  value: string;
  onChange: (value: string) => void;
  /**
   * 개수가 0이어도 남길 값 — `전체`처럼 범위를 여는 칸.
   * 이 값들은 "고를 것이 있는가" 판정에서도 빠진다: 전체 하나만 남은 줄은 손잡이가 아니다.
   */
  alwaysVisible?: string[];
  className?: string;
}

/**
 * 같은 면의 범위를 좁히는 배타적 세그먼트 그룹 (DESIGN.md `components.FilterSegment`).
 *
 * 탭은 면을 바꾸고 세그먼트는 범위를 좁힌다 — 그래서 하단선이 아니라 teal wash 문법을 쓴다.
 * 계약(개수 병기·0건 미렌더·roving tabIndex·←/→/Home/End)을 여기 한 번만 구현한다.
 * 손으로 두 번 구현하면 다음 화면에서 또 갈라진다.
 */
export function FilterSegment({
  label,
  items,
  value,
  onChange,
  alwaysVisible = [],
  className,
}: Props): ReactElement | null {
  const listRef = useRef<HTMLDivElement>(null);

  // 눌러도 아무것도 없는 칸은 손잡이가 아니다. 단 선택된 칸은 비어도 남긴다 —
  // 검색으로 0건이 된 순간 눌린 칸이 사라지면 지금 어디에 서 있는지 알 수 없다.
  const visible = items.filter(
    (item) =>
      item.count > 0 || alwaysVisible.includes(item.value) || item.value === value,
  );
  // `전체`는 범위를 여는 칸이지 고르는 칸이 아니다 — 그것만 남았다면 고를 것이 없다.
  const choices = visible.filter((item) => !alwaysVisible.includes(item.value));
  if (choices.length < 2) return null;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const current = visible.findIndex((item) => item.value === value);
    const last = visible.length - 1;
    const next =
      event.key === "ArrowRight" ? (current + 1) % visible.length
      : event.key === "ArrowLeft" ? (current + last) % visible.length
      : event.key === "Home" ? 0
      : event.key === "End" ? last
      : -1;
    if (next < 0) return;
    event.preventDefault();
    onChange(visible[next].value);
    // 포커스도 함께 옮긴다 — 선택만 움직이면 다음 화살표가 옛 자리에서 출발한다.
    listRef.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  };

  return (
    <div
      ref={listRef}
      role="tablist"
      aria-label={label}
      onKeyDown={onKeyDown}
      className={`flex flex-wrap items-center gap-0.5 w-fit p-0.5 border border-border rounded-md ${className ?? ""}`}
    >
      {visible.map((item) => {
        const on = item.value === value;
        return (
          <button
            key={item.value}
            role="tab"
            type="button"
            title={item.title}
            aria-selected={on}
            tabIndex={on ? 0 : -1}
            className={`h-6 px-2 rounded flex items-center gap-1.5 text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary focus-visible:outline-offset-2 ${
              on
                ? "bg-primary/10 text-primary-bright"
                : "text-text-secondary hover:text-text hover:bg-raised"
            }`}
            onClick={() => onChange(item.value)}
          >
            {item.label}
            {/* 활성 칸에서는 muted를 쓰지 않는다 — teal wash 위에서 읽히지 않는다. */}
            <span className={on ? "text-primary-bright/70" : "text-text-muted"}>
              {item.count.toLocaleString("ko-KR")}
            </span>
          </button>
        );
      })}
    </div>
  );
}
