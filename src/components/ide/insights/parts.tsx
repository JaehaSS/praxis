import type { ReactNode } from "react";

/**
 * 직전 구간 대비 증감 칩. 상태색(초록/빨강)을 쓰지 않는다 —
 * 증가가 늘 좋은 신호는 아니고, 상태색은 상태 표시 전용이다(DESIGN.md).
 */
export function DeltaChip({ delta }: { delta: number | null }) {
  if (delta == null) return null;
  const arrow = delta > 0 ? "▲" : delta < 0 ? "▼" : "=";
  return (
    <span
      className="inline-flex items-center gap-0.5 text-xs text-text-secondary font-code shrink-0"
      title="직전 동일 기간 대비"
    >
      <span aria-hidden>{arrow}</span>
      {Math.abs(delta)}%
    </span>
  );
}

/** 보조 지표 타일. `hero`는 섹션 대표 수치용으로 한 단계 크게. */
export function Metric({
  label,
  value,
  delta,
  hero,
  accent,
}: {
  label: string;
  value: string;
  delta?: number | null;
  hero?: boolean;
  accent?: boolean;
}) {
  return (
    <div className="bg-surface border border-border rounded-lg p-3">
      <div className="text-xs text-text-secondary mb-0.5">{label}</div>
      <div className="flex items-baseline gap-2">
        <span
          className={`${hero ? "text-2xl font-bold" : "text-xl font-medium"} truncate`}
          style={accent ? { color: "var(--c-primary-bright)" } : undefined}
          title={value}
        >
          {value}
        </span>
        {delta !== undefined && <DeltaChip delta={delta ?? null} />}
      </div>
    </div>
  );
}

/** 섹션 제목 + 앵커 타깃. `id`는 스크롤 스파이가 관찰하는 대상이다. */
export function SectionHeader({
  id,
  title,
  hint,
}: {
  id: string;
  title: string;
  hint?: ReactNode;
}) {
  return (
    <div id={id} className="flex items-baseline justify-between mb-3 scroll-mt-20">
      <h2 className="text-lg font-semibold">{title}</h2>
      {hint && <span className="text-xs text-text-muted">{hint}</span>}
    </div>
  );
}

/** 순위 목록용 점유 막대. 중립 회색 — Teal은 히어로 시각화에만 남긴다. */
export function RankBar({ value, max }: { value: number; max: number }) {
  const pct = max > 0 ? Math.max(1, (value / max) * 100) : 0;
  return (
    <div className="h-1.5 rounded-full bg-raised overflow-hidden">
      <div
        className="h-full rounded-full"
        style={{ width: `${pct}%`, background: "var(--c-border-strong)" }}
      />
    </div>
  );
}
