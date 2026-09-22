import type { ReactElement, ReactNode } from "react";

import type { Insights, RetroDigest, TaskPatterns } from "../../../lib/ipc";
import { fmtUsd, totalCost } from "../../../lib/pricing";
import { deltaPct, fmtInt, fmtPct, fmtTokens } from "./format";
import { DeltaChip } from "./parts";

interface Props {
  insights: Insights;
  patterns: TaskPatterns | null;
  retro: RetroDigest | null;
  onJump: (id: string) => void;
}

const pct = (part: number, whole: number) => (whole > 0 ? Math.round((part / whole) * 100) : 0);

/**
 * 질문 레인 하나. **질문이 먼저 오고 답이 두 줄로 따른다** — 이 순서가 A안(질문 축)의
 * 전부다(설계 0054 DR-1).
 */
function Lane({
  question,
  target,
  label,
  onJump,
  children,
}: {
  question: string;
  target: string;
  label: string;
  onJump: (id: string) => void;
  children: ReactNode;
}) {
  return (
    <div className="py-3.5 border-b border-border last:border-0">
      <div className="flex items-baseline justify-between gap-3 mb-1.5">
        <span className="text-sm font-medium">{question}</span>
        <button
          className="text-xs text-text-muted hover:text-primary-bright shrink-0"
          onClick={() => onJump(target)}
        >
          {label} →
        </button>
      </div>
      <div className="text-sm text-text-secondary space-y-0.5">{children}</div>
    </div>
  );
}

/**
 * 요약 — 질문 4레인 (설계 0054 §6.1).
 *
 * 숫자를 먼저 던지지 않는다. 현행 대상 축(활동/분해/스킬/리듬)은 **사용자가 질문을 스스로
 * 만들어야** 숫자가 의미를 가졌다. 질문을 화면이 먼저 제시하면 그 부담이 사라진다.
 */
export function SummaryLanes({ insights, patterns, retro, onJump }: Props): ReactElement {
  const cost = totalCost(insights.models);
  const cacheHit = fmtPct(
    insights.cache_read_tokens,
    insights.cache_read_tokens + insights.cache_creation_tokens + insights.input_tokens,
  );

  const funnel = patterns?.funnel;
  const trend = patterns?.discard_trend ?? [];
  // 추세는 첫 달과 마지막 달만 견준다 — 요약 레인에 곡선을 넣을 자리는 없다.
  const first = trend[0];
  const last = trend.length > 1 ? trend[trend.length - 1] : null;
  const trendText =
    first && last
      ? `${first.month.slice(5)}월 ${pct(first.discarded, first.total)}% → ${last.month.slice(
          5,
        )}월 ${pct(last.discarded, last.total)}%`
      : null;

  const facts = retro?.facts;

  return (
    <div className="bg-surface border border-border rounded-lg px-4">
      <Lane question="얼마나 썼나?" target="cost" label="비용" onJump={onJump}>
        <div className="flex items-baseline gap-2">
          <span>
            {fmtTokens(insights.total_tokens)} 토큰
            {cost && ` · ${fmtUsd(cost.usd)}`}
          </span>
          <DeltaChip delta={deltaPct(insights.total_tokens, insights.prev?.total_tokens)} />
        </div>
        <div className="text-text-muted">캐시 히트 {cacheHit}</div>
      </Lane>

      <Lane question="무엇을 했나?" target="tasks" label="작업" onJump={onJump}>
        {funnel ? (
          <>
            <div>
              작업 {fmtInt(funnel.total)}건 · 승인 {fmtInt(funnel.done)}(
              {pct(funnel.done, funnel.total)}%)
            </div>
            <div className="text-text-muted">
              폐기 {fmtInt(funnel.discarded)}({pct(funnel.discarded, funnel.total)}%)
              {trendText && ` · ${trendText}`}
            </div>
          </>
        ) : (
          <div className="text-text-muted">집계 중…</div>
        )}
      </Lane>

      <Lane question="어떻게 일했나?" target="how" label="방식" onJump={onJump}>
        {patterns ? (
          <>
            <div>
              {pct(patterns.followup.with_followup, patterns.followup.total)}%가 후속 입력을
              필요로 했다
            </div>
            <div className="text-text-muted">
              최다 역할{" "}
              {patterns.role_outcomes[0]
                ? `${patterns.role_outcomes[0].role} · 승인 ${pct(
                    patterns.role_outcomes[0].done,
                    patterns.role_outcomes[0].count,
                  )}%`
                : "—"}
            </div>
          </>
        ) : (
          <div className="text-text-muted">집계 중…</div>
        )}
      </Lane>

      <Lane question="무엇을 바꿀까?" target="retro" label="회고" onJump={onJump}>
        {facts && retro ? (
          <>
            <div>
              제안 {fmtInt(facts.proposals_pending)}건 적체 · 채택{" "}
              {fmtInt(facts.proposals_applied)}건
            </div>
            <div className="text-text-muted">
              {new Date(retro.week_start * 1000).toLocaleDateString()} 주 회고
            </div>
          </>
        ) : (
          <div className="text-text-muted">아직 생성된 회고가 없습니다.</div>
        )}
      </Lane>
    </div>
  );
}
