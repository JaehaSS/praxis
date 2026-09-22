import { useCallback, useEffect, useState } from "react";
import type { Schedule } from "../lib/ipc";
import { ago, until } from "../lib/fmt";
import { api } from "./api";
import { Button, Empty, Spinner } from "./primitives";

// 스케줄·리마인더. (설계 0013 §5.3)
// cron 편집은 폰에서 오조작 위험이 크다 — 여기서는 **보기·끄기·삭제**와, 폰에서 실제로
// 자주 쓰는 1회성 리마인더 추가만 제공한다. 새 cron 스케줄은 데스크톱에서 만든다.

/** 몇 분 뒤 알림 — 손가락으로 한 번에 고를 수 있는 값만 남긴다. */
const DELAY_PRESETS = [10, 30, 60, 180];

function describeSchedule(schedule: Schedule): string {
  if (schedule.run_at != null) {
    const remaining = schedule.run_at - Math.floor(Date.now() / 1000);
    return remaining > 0 ? `${until(schedule.run_at)} 후 실행` : "실행 시각 지남";
  }
  return schedule.cron;
}

export function SchedulesScreen() {
  const [schedules, setSchedules] = useState<Schedule[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [text, setText] = useState("");
  const [delay, setDelay] = useState(DELAY_PRESETS[0]);

  const load = useCallback(() => {
    setError(null);
    api
      .scheduleList()
      .then(setSchedules)
      .catch((cause: unknown) => {
        setSchedules([]);
        setError(cause instanceof Error ? cause.message : String(cause));
      });
  }, []);

  useEffect(() => load(), [load]);

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      load();
    } catch (cause: unknown) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const addReminder = () => {
    const trimmed = text.trim();
    if (!trimmed) return;
    void run(async () => {
      await api.reminderAdd(trimmed, delay);
      setText("");
    });
  };

  if (!schedules) return <Spinner label="스케줄을 불러오는 중" />;

  return (
    <div className="space-y-5 py-4">
      <section className="space-y-2 px-4">
        <div className="text-xs text-text-muted">리마인더 추가</div>
        <input
          value={text}
          onChange={(event) => setText(event.target.value)}
          aria-label="리마인더 내용"
          placeholder="무엇을 알릴까요"
          className="min-h-[44px] w-full rounded-lg border border-border bg-surface px-3 text-sm text-text placeholder:text-text-muted"
        />
        <div className="flex flex-wrap gap-2">
          {DELAY_PRESETS.map((minutes) => (
            <button
              key={minutes}
              type="button"
              onClick={() => setDelay(minutes)}
              aria-pressed={delay === minutes}
              className={`min-h-[44px] rounded-lg border px-3 text-sm ${
                delay === minutes
                  ? "border-primary text-primary-bright"
                  : "border-border text-text-muted"
              }`}
            >
              {minutes < 60 ? `${minutes}분` : `${minutes / 60}시간`}
            </button>
          ))}
        </div>
        <Button disabled={busy || text.trim().length === 0} onClick={addReminder}>
          추가
        </Button>
      </section>

      {error ? <div className="px-4 text-sm text-status-failed">{error}</div> : null}

      <section>
        <div className="px-4 pb-2 text-xs text-text-muted">등록된 스케줄</div>
        {schedules.length === 0 ? (
          <Empty>등록된 스케줄이 없습니다.</Empty>
        ) : (
          <ul className="divide-y divide-border">
            {schedules.map((schedule) => (
              <li key={schedule.id} className="space-y-2 px-4 py-3">
                <div className="flex items-start gap-2">
                  <div className="min-w-0 flex-1">
                    <div className="text-sm text-text">{schedule.label}</div>
                    <div className="font-code text-xs text-text-muted">
                      {describeSchedule(schedule)}
                    </div>
                  </div>
                  <span
                    className={`shrink-0 text-xs ${
                      schedule.enabled ? "text-status-done" : "text-text-muted"
                    }`}
                  >
                    {schedule.enabled ? "켜짐" : "꺼짐"}
                  </span>
                </div>
                <div className="flex items-center gap-2 text-xs text-text-muted">
                  {schedule.last_run_at ? <span>마지막 {ago(schedule.last_run_at)} 전</span> : null}
                  <div className="ml-auto flex gap-2">
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() =>
                        void run(() => api.scheduleSetEnabled(schedule.id, !schedule.enabled))
                      }
                      className="min-h-[36px] rounded-md border border-border px-3 text-text disabled:opacity-50"
                    >
                      {schedule.enabled ? "끄기" : "켜기"}
                    </button>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void run(() => api.scheduleRemove(schedule.id))}
                      className="min-h-[36px] rounded-md border border-border px-3 text-status-failed disabled:opacity-50"
                    >
                      삭제
                    </button>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
