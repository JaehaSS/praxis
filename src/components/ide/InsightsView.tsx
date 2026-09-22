import { useCallback, useEffect, useMemo, useRef, useState, type ReactElement } from "react";
import {
  insightsCompute,
  retroDigestGet,
  taskPatterns,
  type Insights,
  type InsightsRange,
  type RetroDigest,
  type TaskPatterns,
} from "../../lib/ipc";
import { modelCost, totalCost, fmtUsd } from "../../lib/pricing";
import { AgentSkillPanel } from "./insights/AgentSkillPanel";
import { AreaChart } from "./insights/AreaChart";
import { BacklogPanel } from "./insights/BacklogPanel";
import { Heatmap } from "./insights/Heatmap";
import { Punchcard } from "./insights/Punchcard";
import { PlanCalendar } from "./insights/PlanCalendar";
import { RetroPanel } from "./insights/RetroPanel";
import { StackedBar } from "./insights/StackedBar";
import { SummaryLanes } from "./insights/SummaryLanes";
import { TaskPatternPanel } from "./insights/TaskPatternPanel";
import { DeltaChip, Metric, RankBar, SectionHeader } from "./insights/parts";
import { OutcomeInsightsPanel } from "./OutcomeInsightsPanel";
import {
  deltaPct,
  fmtHour,
  fmtInt,
  fmtPct,
  fmtTokens,
  prettyModel,
  relativeDay,
} from "./insights/format";

const RANGES: { v: InsightsRange; label: string }[] = [
  { v: "all", label: "전체" },
  { v: "30d", label: "30d" },
  { v: "7d", label: "7d" },
];

/**
 * 질문 축 5섹션 (설계 0054 DR-1).
 *
 * 이전에는 대상 축 6섹션(활동/분해/스킬/리듬/계획/AX 결과)이었다. 대상으로 나누면
 * **사용자가 질문을 스스로 만들어야** 숫자가 의미를 갖는다 — 그 부담을 화면으로 옮겼다.
 */
const SECTIONS = [
  { id: "summary", label: "요약" },
  { id: "cost", label: "지출" },
  { id: "tasks", label: "작업" },
  { id: "how", label: "방식" },
  { id: "retro", label: "회고" },
];

/** 캐시 히트율 — 읽어온 캐시가 전체 입력성 토큰에서 차지하는 비율. */
const cacheHitRate = (m: { cache_read_tokens: number; cache_creation_tokens: number; input_tokens: number }) =>
  fmtPct(m.cache_read_tokens, m.cache_read_tokens + m.cache_creation_tokens + m.input_tokens);

interface InsightsViewProps {
  onOpenMemory?: () => void;
  /** 메모리 › 자기개선 탭을 여는 통로 — 제안 승인·거부는 거기서만 한다(ADR 0191). */
  onOpenSelfImprove?: () => void;
  /** 계획 섹션에서 정본 편집처(Home)로 보내는 링크. 없으면 링크를 숨긴다. */
  onOpenHome?: () => void;
  /** 회고를 읽었을 때 — 사이드바 신선도 점을 끄는 통로(설계 0054 DR-6). */
  onRetroSeen?: (weekStart: number) => void;
}

/** 사용량 인사이트 — 요약 → 지출 → 작업 → 방식 → 회고 순의 단일 스크롤 리포트. */
export function InsightsView({
  onOpenMemory,
  onOpenSelfImprove,
  onOpenHome,
  onRetroSeen,
}: InsightsViewProps = {}): ReactElement {
  const [range, setRange] = useState<InsightsRange>("all");
  const [data, setData] = useState<Insights | null>(null);
  const [patterns, setPatterns] = useState<TaskPatterns | null>(null);
  const [retro, setRetro] = useState<RetroDigest | null>(null);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [active, setActive] = useState(SECTIONS[0].id);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setErr(null);
    insightsCompute(range)
      .then((d) => alive && setData(d))
      .catch((e) => alive && setErr(String(e)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [range]);

  // 작업 패턴은 트랜스크립트가 아니라 작업 DB를 읽는다 — 위 집계가 실패해도 살아 있다.
  useEffect(() => {
    let alive = true;
    taskPatterns(range)
      .then((p) => alive && setPatterns(p))
      .catch(() => alive && setPatterns(null));
    return () => {
      alive = false;
    };
  }, [range]);

  // 회고는 범위 칩과 무관하다 — 주 단위로 고정된 산출물이라 한 번만 읽는다.
  useEffect(() => {
    let alive = true;
    retroDigestGet(null)
      .then((d) => alive && setRetro(d))
      .catch(() => alive && setRetro(null));
    return () => {
      alive = false;
    };
  }, []);

  // 스크롤 스파이 — 단일 스크롤에서도 "지금 어느 섹션인지"를 헤더가 알려준다.
  useEffect(() => {
    const root = scrollRef.current;
    if (!root) return;
    const obs = new IntersectionObserver(
      (entries) => {
        const shown = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (shown[0]) setActive(shown[0].target.id);
      },
      { root, rootMargin: "-72px 0px -55% 0px", threshold: 0 },
    );
    for (const s of SECTIONS) {
      const el = root.querySelector(`#${s.id}`);
      if (el) obs.observe(el);
    }
    return () => obs.disconnect();
  }, [data, patterns, retro]);

  const jump = useCallback((id: string) => {
    scrollRef.current?.querySelector(`#${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, []);

  const cost = useMemo(() => (data ? totalCost(data.models) : null), [data]);
  const maxProjectTokens = useMemo(
    () => (data ? Math.max(1, ...data.projects.map((p) => p.total_tokens)) : 1),
    [data],
  );
  const weekendPct = useMemo(() => {
    if (!data || data.weekday_hours.length < 168) return "—";
    const total = data.weekday_hours.reduce((sum, c) => sum + c, 0);
    const weekend = data.weekday_hours
      .slice(0, 24)
      .concat(data.weekday_hours.slice(6 * 24, 7 * 24))
      .reduce((sum, c) => sum + c, 0);
    return fmtPct(weekend, total);
  }, [data]);

  const chip = (on: boolean) =>
    `h-7 px-3 rounded-md text-sm transition-colors ${
      on ? "bg-raised text-primary-bright" : "text-text-secondary hover:text-text"
    }`;

  /** 트랜스크립트 기반 섹션(요약·지출·방식)이 쓸 수 있는 상태인가. */
  const hasUsage = data != null && data.messages > 0;

  return (
    <div ref={scrollRef} className="flex-1 overflow-auto">
      {/* 반투명 + blur는 표 숫자가 비쳐 읽기를 방해한다 — 데이터 화면에선 불투명이 낫다. */}
      <div className="sticky top-0 z-10 bg-bg border-b border-border">
        <div className="max-w-5xl mx-auto px-7 h-14 flex items-center justify-between gap-4">
          <span className="text-sm text-text-secondary shrink-0 hidden sm:block">인사이트</span>
          <div className="flex items-center gap-1">
            {SECTIONS.map((s) => (
              <button key={s.id} className={chip(active === s.id)} onClick={() => jump(s.id)}>
                {s.label}
              </button>
            ))}
          </div>
          <div className="flex items-center gap-1 bg-surface border border-border rounded-md p-0.5 shrink-0">
            {RANGES.map((r) => (
              <button
                key={r.v}
                onClick={() => setRange(r.v)}
                className={`h-6 px-2.5 rounded text-sm ${
                  range === r.v ? "bg-raised text-text" : "text-text-secondary hover:text-text"
                }`}
              >
                {r.label}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="max-w-5xl mx-auto px-7 pb-20">
        {err && <div className="text-status-failed text-sm mt-6 font-code">{err}</div>}

        {/* ── 요약 ──────────────────────────────
            질문 4레인. 셋 다 갖춰지기 전에는 레인이 각자 "집계 중"을 띄운다 — 요약 때문에
            아래 섹션 전체가 막히면 안 된다. */}
        <section className="pt-6">
          <SectionHeader id="summary" title="요약" hint="질문 넷" />
          {loading && !data ? (
            <div className="text-text-muted text-sm py-8 text-center">집계 중…</div>
          ) : data ? (
            <SummaryLanes insights={data} patterns={patterns} retro={retro} onJump={jump} />
          ) : (
            <div className="text-text-muted text-sm border border-border rounded-lg p-6">
              사용량 집계를 읽지 못했습니다.
            </div>
          )}
        </section>

        {/* ── 지출 ────────────────────────────── */}
        <section className="pt-12">
          <SectionHeader
            id="cost"
            title="지출"
            hint={
              hasUsage
                ? `입력 ${fmtTokens(data.input_tokens)} · 출력 ${fmtTokens(data.output_tokens)}`
                : undefined
            }
          />
          {!hasUsage ? (
            <div className="text-text-muted text-sm border border-border rounded-lg p-8 text-center">
              이 기간에 사용 기록이 없습니다.
            </div>
          ) : (
            <>
              <div className="bg-surface border border-border rounded-lg p-4 mb-2.5">
                <div className="flex items-start justify-between gap-4 mb-3">
                  <div>
                    <div className="text-xs text-text-secondary">총 토큰</div>
                    <div className="flex items-baseline gap-2">
                      <span className="text-2xl font-bold" style={{ color: "var(--c-primary-bright)" }}>
                        {fmtTokens(data.total_tokens)}
                      </span>
                      <DeltaChip delta={deltaPct(data.total_tokens, data.prev?.total_tokens)} />
                    </div>
                  </div>
                  <div className="flex items-center gap-3 text-xs text-text-muted pt-1">
                    <span className="flex items-center gap-1.5">
                      <span
                        className="w-2.5 h-2.5 rounded-sm"
                        style={{ background: "var(--c-primary-bright)" }}
                        aria-hidden
                      />
                      토큰
                    </span>
                    <span className="flex items-center gap-1.5">
                      <span className="w-2.5 border-t border-dashed border-text-muted" aria-hidden />
                      메시지
                    </span>
                  </div>
                </div>
                <AreaChart days={data.days} />
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-2.5 mb-2.5">
                <Metric
                  label="세션"
                  value={fmtInt(data.sessions)}
                  delta={deltaPct(data.sessions, data.prev?.sessions)}
                />
                <Metric
                  label="메시지"
                  value={fmtInt(data.messages)}
                  delta={deltaPct(data.messages, data.prev?.messages)}
                />
                <Metric
                  label="활성 일수"
                  value={fmtInt(data.active_days)}
                  delta={deltaPct(data.active_days, data.prev?.active_days)}
                />
                <Metric
                  label={cost?.partial ? "예상 비용 (일부 미산정)" : "예상 비용"}
                  value={cost ? fmtUsd(cost.usd) : "—"}
                />
              </div>

              <div className="bg-surface border border-border rounded-lg p-4 mb-2.5">
                <div className="text-xs text-text-secondary mb-2">모델 점유율 (토큰)</div>
                <StackedBar
                  segments={data.models.map((m) => ({
                    key: m.model,
                    label: prettyModel(m.model),
                    value: m.total_tokens,
                  }))}
                />
              </div>

              <div className="bg-surface border border-border rounded-lg overflow-x-auto mb-2.5">
                <table className="w-full text-sm min-w-[520px]">
                  <thead>
                    <tr className="text-xs text-text-secondary border-b border-border">
                      <th className="text-left font-medium px-3 py-2">모델</th>
                      <th className="text-right font-medium px-3 py-2">토큰</th>
                      <th className="text-right font-medium px-3 py-2">캐시 히트</th>
                      <th className="text-right font-medium px-3 py-2">세션</th>
                      <th className="text-right font-medium px-3 py-2">메시지</th>
                      <th className="text-right font-medium px-3 py-2">비용</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.models.map((m) => {
                      const c = modelCost(m);
                      return (
                        <tr key={m.model} className="border-b border-border last:border-0">
                          <td className="px-3 py-2 truncate max-w-[180px]" title={m.model}>
                            {prettyModel(m.model)}
                          </td>
                          <td className="px-3 py-2 text-right font-code">
                            {fmtTokens(m.total_tokens)}
                          </td>
                          <td className="px-3 py-2 text-right font-code text-text-secondary">
                            {cacheHitRate(m)}
                          </td>
                          <td className="px-3 py-2 text-right font-code text-text-secondary">
                            {fmtInt(m.sessions)}
                          </td>
                          <td className="px-3 py-2 text-right font-code text-text-secondary">
                            {fmtInt(m.messages)}
                          </td>
                          <td className="px-3 py-2 text-right font-code">
                            {c == null ? "—" : fmtUsd(c)}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>

              <div className="bg-surface border border-border rounded-lg">
                <div className="text-xs text-text-secondary px-3 pt-3 pb-1">프로젝트</div>
                {data.projects.length === 0 ? (
                  <div className="text-text-muted text-sm px-3 pb-3">프로젝트 정보가 없습니다.</div>
                ) : (
                  data.projects.map((p) => (
                    <div key={p.path} className="px-3 py-2 border-t border-border">
                      <div className="flex items-baseline justify-between gap-3 mb-1.5">
                        <span className="truncate" title={p.path}>
                          {p.name}
                        </span>
                        <span className="text-xs font-code text-text-secondary shrink-0">
                          {fmtTokens(p.total_tokens)} · {fmtInt(p.sessions)}세션 ·{" "}
                          {relativeDay(p.last_active)}
                        </span>
                      </div>
                      <RankBar value={p.total_tokens} max={maxProjectTokens} />
                    </div>
                  ))
                )}
              </div>
            </>
          )}
        </section>

        {/* ── 작업 ──────────────────────────────
            작업 DB만 읽는다. 위 사용량 리포트가 비어 있어도 여기는 채워진다. */}
        <section className="pt-12">
          <SectionHeader id="tasks" title="작업" hint="작업 DB · 트랜스크립트 비의존" />
          <TaskPatternPanel data={patterns} />

          {/* 계획은 사용량이 아니라 day_items를 읽으므로 range 칩과 독립이다(설계 0023).
              작업 섹션 안에 두는 이유는 "무엇을 했나"의 다른 얼굴이기 때문이다. */}
          <div className="pt-8">
            <div className="text-xs text-text-secondary mb-3">계획 · 읽기 전용</div>
            <PlanCalendar onOpenHome={onOpenHome} />
            {/* 백로그는 캘린더에 도트로 찍히지 않는다(날짜가 없으니까). 그래서 유일하게
                안 보이는 계획이 되고, 안 보이는 계획은 무덤이 된다 (플랜 0054). */}
            <BacklogPanel onOpenHome={onOpenHome} />
          </div>
        </section>

        {/* ── 방식 ────────────────────────────── */}
        <section className="pt-12">
          <SectionHeader id="how" title="방식" hint="스킬 · 리듬" />
          <AgentSkillPanel range={range} />

          {hasUsage && (
            <div className="pt-8">
              <div className="bg-surface border border-border rounded-lg p-4 mb-2.5">
                <div className="text-xs text-text-secondary mb-3">요일 × 시간</div>
                <Punchcard weekdayHours={data.weekday_hours} />
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-2.5 mb-2.5">
                <Metric label="현재 연속" value={`${data.current_streak}일`} />
                <Metric label="최장 연속" value={`${data.longest_streak}일`} />
                <Metric label="최다 사용 시간" value={fmtHour(data.peak_hour)} />
                <Metric label="주말 비중" value={weekendPct} />
              </div>
              <div className="bg-surface border border-border rounded-lg p-4">
                <div className="text-xs text-text-secondary mb-3">일별 활동</div>
                <Heatmap days={data.days} range={range} />
              </div>
            </div>
          )}

          {/* AX 결과는 접어둔다. goal_contract를 쓴 작업이 사실상 없어 지표 다수가 0에
              수렴하지만, 지표가 0인 것과 기능이 필요 없는 것은 다르다(설계 0054 DR-8). */}
          <details className="mt-8 border border-border rounded-lg bg-surface">
            <summary className="cursor-pointer select-none px-4 py-2.5 text-sm text-text-secondary">
              AX 결과 — 작업 DB 기반 채택·성과 지표
            </summary>
            <div className="px-4 pb-4">
              <OutcomeInsightsPanel range={range} onOpenMemory={onOpenMemory} />
            </div>
          </details>
        </section>

        {/* ── 회고 ──────────────────────────────
            범위 칩과 무관하다 — 주 단위로 고정된 산출물이다. */}
        <section className="pt-12">
          <SectionHeader id="retro" title="회고" hint="주간 · 서술은 생성, 수치는 집계" />
          <RetroPanel initial={retro} onSeen={onRetroSeen} onOpenSelfImprove={onOpenSelfImprove} />
        </section>
      </div>
    </div>
  );
}
