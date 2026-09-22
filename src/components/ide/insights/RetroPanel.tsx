import { useEffect, useState, type ReactElement } from "react";

import { retroDigestGet, retroDigestList, type RetroDigest } from "../../../lib/ipc";
import { saveSeenWeek } from "../../../lib/retro-seen";
import { fmtInt } from "./format";
import { Metric } from "./parts";

interface Props {
  /** 최신 주 다이제스트. 상위가 이미 로드한 값을 내려받아 같은 조회를 두 번 하지 않는다. */
  initial: RetroDigest | null;
  /** 회고를 실제로 읽었을 때 — 사이드바 신선도 점을 끄는 통로. */
  onSeen?: (weekStart: number) => void;
  /** 메모리 › 자기개선 탭을 여는 통로 — 승인·거부는 거기서만 한다(ADR 0191). */
  onOpenSelfImprove?: () => void;
}

/** `2026-08-24 주` — 몇 주차인지는 정의가 갈리므로 시작 날짜를 그대로 쓴다. */
function weekLabel(weekStart: number): string {
  const date = new Date(weekStart * 1000);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(
    date.getDate(),
  ).padStart(2, "0")} 주`;
}

const ymd = (epoch: number) => new Date(epoch * 1000).toLocaleDateString();

/**
 * 주간 회고 (설계 0054 §6.4).
 *
 * **서술은 LLM이 쓰지만 숫자는 아니다.** 화면에 뜨는 수치는 `facts`이고, 그것은 Rust가
 * SQL로 계산해 프롬프트에 넣은 원본이다(DR-7). "출처"를 펼치면 그 원본이 보인다.
 *
 * 제안 승인·거부는 여기서 하지 않는다 — 숫자와 "자기개선에서 검토" 링크만 남기고,
 * 결정은 메모리 › 자기개선 탭이 맡는다(ADR 0191).
 */
export function RetroPanel({ initial, onSeen, onOpenSelfImprove }: Props): ReactElement {
  const [digest, setDigest] = useState<RetroDigest | null>(initial);
  const [weeks, setWeeks] = useState<number[]>([]);
  const [cursor, setCursor] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showFacts, setShowFacts] = useState(false);

  // 최신 주를 열었다는 사실을 기록한다 — 다시 열었다고 점이 켜지지 않도록 최신 주만 올린다.
  useEffect(() => {
    if (!initial) return;
    saveSeenWeek(initial.week_start);
    onSeen?.(initial.week_start);
  }, [initial, onSeen]);

  // 주를 옮길 때만 다시 조회한다. 최초 렌더는 상위가 준 `initial`을 쓴다.
  useEffect(() => {
    // 최신 주로 되돌아온 경우 — 상위가 이미 가진 값으로 복원한다. 여기서 그냥 빠져나가면
    // 화면이 지난주에 머문 채로 남는다.
    if (cursor == null) {
      setDigest(initial);
      return;
    }
    let alive = true;
    setLoading(true);
    setError(null);
    retroDigestGet(cursor)
      .then((result) => alive && setDigest(result))
      .catch((reason: unknown) => alive && setError(String(reason)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [cursor, initial]);

  useEffect(() => {
    retroDigestList(52)
      .then((list) => setWeeks(list.map((w) => w.week_start)))
      .catch(() => setWeeks([]));
  }, []);

  const current = digest?.week_start ?? null;
  const older = weeks.find((w) => current != null && w < current) ?? null;
  const newer = [...weeks].reverse().find((w) => current != null && w > current) ?? null;
  const facts = digest?.facts;

  return (
    <>
      <div className="flex items-center justify-between gap-3 mb-3">
        <span className="text-sm text-text-secondary">
          {digest ? weekLabel(digest.week_start) : "생성된 회고 없음"}
        </span>
        <div className="flex items-center gap-1">
          <button
            className="h-7 px-2.5 rounded-md text-sm text-text-secondary hover:text-text disabled:opacity-40"
            disabled={older == null}
            onClick={() => setCursor(older)}
          >
            ◀ 지난주
          </button>
          <button
            className="h-7 px-2.5 rounded-md text-sm text-text-secondary hover:text-text disabled:opacity-40"
            disabled={newer == null && cursor == null}
            onClick={() => setCursor(newer)}
          >
            다음주 ▶
          </button>
        </div>
      </div>

      {error && <div className="text-status-failed text-sm font-code mb-2.5">{error}</div>}

      {loading ? (
        <div className="text-text-muted text-sm py-8">불러오는 중…</div>
      ) : !digest ? (
        <div className="text-text-muted text-sm border border-border rounded-lg p-6 mb-2.5">
          아직 생성된 회고가 없습니다. 스케줄 화면의 &lsquo;주간 회고&rsquo;가 켜져 있으면 다음
          월요일에 지난주 회고가 승인 대기로 올라옵니다.
        </div>
      ) : (
        <div className="bg-surface border border-border rounded-lg p-4 mb-2.5">
          <div className="text-xs text-text-secondary mb-2">이번 주 이렇게 일했습니다</div>
          {digest.body.split(/\n{2,}/).map((para, index) => (
            <p key={index} className="text-sm leading-relaxed mb-2 last:mb-0 whitespace-pre-wrap">
              {para}
            </p>
          ))}
          <button
            className="mt-3 text-xs text-text-muted hover:text-text-secondary"
            onClick={() => setShowFacts((on) => !on)}
          >
            출처 {showFacts ? "▾" : "▸"}
          </button>
          {showFacts && facts && (
            <div className="mt-2 border-t border-border pt-2">
              {/* 서술이 아니라 이 값이 정본이다 — 문장이 숫자를 틀렸는지 여기서 대조한다. */}
              <div className="text-xs text-text-muted mb-2">
                서술에 쓰인 확정 수치. 이 값은 LLM이 만든 것이 아니라 작업 DB에서 계산됐습니다.
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-2.5">
                <Metric label="작업" value={fmtInt(facts.tasks_total)} />
                <Metric label="승인" value={fmtInt(facts.tasks_done)} />
                <Metric label="폐기율" value={`${facts.discard_rate_pct}%`} />
                <Metric label="후속 입력" value={`${facts.followup_pct}%`} />
              </div>
              <div className="text-xs text-text-muted mt-2 font-code">
                생성 {ymd(digest.generated_at)}
                {digest.agent && ` · ${digest.agent}`}
              </div>
            </div>
          )}
        </div>
      )}

      {/* ── 자기개선 루프 ──
          "검토 대기 3건"처럼 순화하지 않는다. 적체와 채택률을 그대로 쓰는 것이 이
          섹션의 존재 이유다(설계 0054 §6.4). */}
      {facts && (
        <div className="bg-surface border border-border rounded-lg p-4 mb-2.5">
          <div className="text-xs text-text-secondary mb-2">자기개선 루프</div>
          <p className="text-sm">
            제안 <span className="font-code">{fmtInt(facts.proposals_pending)}</span>건이 검토를
            기다립니다 · 채택 <span className="font-code">{fmtInt(facts.proposals_applied)}</span>건
          </p>
          {facts.proposals_pending > 0 && facts.proposals_applied === 0 && (
            <p className="text-xs text-text-muted mt-1.5">
              쌓이기만 하고 하나도 채택되지 않았습니다. 제안이 나쁜 것인지 검토 비용이 큰
              것인지는 이 화면이 답하지 못합니다.
            </p>
          )}
          {facts.proposals_pending > 0 && onOpenSelfImprove && (
            <button
              className="mt-2 text-xs text-primary-bright hover:underline"
              onClick={onOpenSelfImprove}
            >
              자기개선에서 검토
            </button>
          )}
        </div>
      )}
    </>
  );
}
