import { useCallback, useEffect, useRef, useState, type ReactElement } from "react";

import {
  quizAnswer,
  quizNext,
  quizReport,
  type QuizAnswerResult,
  type QuizItem,
} from "../lib/ipc";

interface Props {
  /**
   * 기다리던 응답이 도착했다.
   *
   * **패널을 닫지 않는다** — 배지로만 알린다(설계 0044 DR-3). 자동으로 닫으면 풀던 문제가
   * 매번 중도 절단되고, 그게 "퀴즈는 성가시다"로 굳는다.
   */
  responseArrived: boolean;
  /** 검수 대기 건수. 0이면 통로를 숨긴다 — 눌러 봐야 빈 목록이다. */
  pendingReview: number;
  onClose: () => void;
  /** 검수 화면으로. 검수만 남았을 때는 게이트가 그쪽으로 바로 열지만, 문제를 푸는 중에
   *  건너가는 통로는 여기뿐이다. */
  onReview: () => void;
}

type Phase = "loading" | "asking" | "answered" | "exhausted" | "error";

export function QuizPanel({
  responseArrived,
  pendingReview,
  onClose,
  onReview,
}: Props): ReactElement {
  const [item, setItem] = useState<QuizItem | null>(null);
  const [result, setResult] = useState<QuizAnswerResult | null>(null);
  const [phase, setPhase] = useState<Phase>("loading");
  const [typed, setTyped] = useState("");
  /** 이 패널에서 하나라도 풀었는가. 빈 큐가 "다 풀었다"인지 "처음부터 없었다"인지 가른다. */
  const solvedOne = useRef(false);

  const load = useCallback(async () => {
    setPhase("loading");
    setResult(null);
    setTyped("");
    try {
      const next = await quizNext();
      setItem(next);
      if (next) {
        setPhase("asking");
      } else if (solvedOne.current) {
        setPhase("exhausted");
      } else {
        // 게이트가 "낼 문제가 있다"를 보고 열었는데 그새 사라졌다. 드문 경쟁이고 여기서
        // 할 말이 없으므로 조용히 닫는다 — 빈 화면을 띄우지 않는 것이 이 수정의 요지다
        // (이슈 #87).
        onClose();
      }
    } catch {
      setPhase("error");
    }
  }, [onClose]);

  useEffect(() => {
    void load();
  }, [load]);

  const submit = async (picked: string) => {
    if (!item || !picked.trim()) return;
    try {
      const outcome = await quizAnswer(item.id, picked);
      setResult(outcome);
      solvedOne.current = true;
      setPhase("answered");
    } catch {
      setPhase("error");
    }
  };

  const report = async () => {
    if (!item) return;
    try {
      await quizReport(item.id);
    } catch {
      // 신고 실패는 조용히 넘긴다 — 다음 문제로 가는 것이 사용자가 원한 결과다.
    }
    void load();
  };

  return (
    <section
      aria-label="대기 퀴즈"
      className="flex w-full flex-col gap-3 rounded-lg border border-border bg-surface p-4 text-sm text-text"
    >
      <header className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className="text-text-secondary">대기 퀴즈</span>
          {item ? (
            <span className="rounded bg-raised px-1.5 py-0.5 text-[11px] text-text-muted">
              {item.kind}
            </span>
          ) : null}
        </div>
        <div className="flex items-center gap-2">
          {responseArrived ? (
            <span className="rounded bg-status-done/15 px-2 py-0.5 text-[11px] text-status-done">
              응답 도착
            </span>
          ) : null}
          {pendingReview > 0 ? (
            <button
              type="button"
              onClick={onReview}
              className="rounded px-2 py-0.5 text-[11px] text-text-muted hover:bg-raised"
            >
              검수 {pendingReview}
            </button>
          ) : null}
          <button
            type="button"
            onClick={onClose}
            className="rounded px-2 py-0.5 text-[11px] text-text-muted hover:bg-raised"
          >
            닫기
          </button>
        </div>
      </header>

      {phase === "loading" ? <p className="text-text-muted">문제를 가져오는 중…</p> : null}

      {phase === "error" ? (
        <div className="flex items-center justify-between gap-2">
          <p className="text-status-failed">문제를 가져오지 못했습니다.</p>
          <button type="button" onClick={() => void load()} className="rounded bg-raised px-2 py-1 text-[11px]">
            다시 시도
          </button>
        </div>
      ) : null}

      {phase === "exhausted" ? (
        <p className="leading-relaxed text-text-muted">준비된 문제를 다 풀었습니다.</p>
      ) : null}

      {item && (phase === "asking" || phase === "answered") ? (
        <>
          <p className="leading-relaxed">{item.question}</p>

          {item.source_excerpt ? (
            <blockquote className="border-l-2 border-border-strong pl-3 text-[12px] leading-relaxed text-text-muted">
              {item.source_excerpt}
            </blockquote>
          ) : null}

          {phase === "asking" ? (
            item.choices ? (
              <div className="flex flex-col gap-1.5">
                {item.choices.map((choice) => (
                  <button
                    key={choice}
                    type="button"
                    onClick={() => void submit(choice)}
                    className="rounded border border-border bg-raised px-3 py-2 text-left hover:border-primary"
                  >
                    {choice}
                  </button>
                ))}
              </div>
            ) : (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void submit(typed);
                }}
                className="flex gap-2"
              >
                <input
                  value={typed}
                  onChange={(e) => setTyped(e.target.value)}
                  placeholder="답을 입력하세요"
                  className="flex-1 rounded border border-border bg-raised px-3 py-2 outline-none focus:border-primary"
                />
                <button type="submit" className="rounded bg-primary px-3 py-2 text-bg">
                  확인
                </button>
              </form>
            )
          ) : null}

          {phase === "answered" && result ? (
            <div className="flex flex-col gap-2">
              <p className={result.correct ? "text-status-done" : "text-status-failed"}>
                {result.correct ? "정답입니다." : `틀렸습니다 — 정답은 ${result.answer}`}
              </p>
              {result.explanation ? (
                <p className="leading-relaxed text-text-secondary">{result.explanation}</p>
              ) : null}
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => void load()}
                  className="rounded bg-raised px-3 py-1.5 text-[12px] hover:bg-border"
                >
                  다음 문제
                </button>
                <button
                  type="button"
                  onClick={() => void report()}
                  className="rounded px-2 py-1.5 text-[11px] text-text-muted hover:text-status-failed"
                >
                  이 문제 신고
                </button>
              </div>
            </div>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
