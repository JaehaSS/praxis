import type { ReactElement, ReactNode } from "react";

interface Props {
  /**
   * `accent`는 "이 항목이 기본을 덮는다"는 신호 — 프로젝트 스킬이 글로벌을 덮고,
   * 항상 적용이 관련성 게이트를 덮는다. 그 외는 전부 `default`다.
   */
  tone?: "default" | "accent";
  title?: string;
  children: ReactNode;
}

/**
 * 상태가 아닌 분류·범위·정책을 다는 태그 (DESIGN.md `components.MetaTag`).
 *
 * Badge는 작업 상태 4종 전용이다 — 그것을 비상태 표시에 끌어 쓰면 색이 상태를 말한다는
 * 계약이 흐려진다. 여기는 채우지 않고 테두리로만 구분한다.
 */
export function MetaTag({ tone = "default", title, children }: Props): ReactElement {
  return (
    <span
      title={title}
      className={`shrink-0 rounded border border-border px-1.5 py-0.5 text-xs ${
        tone === "accent" ? "text-primary-bright" : "text-text-muted"
      }`}
    >
      {children}
    </span>
  );
}
