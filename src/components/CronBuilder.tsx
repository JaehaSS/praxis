import { useState, useEffect } from "react";
import { cronNextRuns } from "../lib/ipc";
import { inputCls } from "./ide/formStyles";

interface Props {
  cron: string;
  onCronChange: (cron: string) => void;
  tz_offset_secs: number;
}

const TEMPLATES = [
  { label: "매일 09:00", cron: "0 0 9 * * *" },
  { label: "매일 18:00", cron: "0 0 18 * * *" },
  { label: "매주 월 09:00", cron: "0 0 9 * * 1" },
  { label: "매달 1일 09:00", cron: "0 0 9 1 * *" },
  { label: "매 시간", cron: "0 0 * * * *" },
  { label: "매 30분", cron: "0 */30 * * * *" },
];

export function CronBuilder({ cron, onCronChange, tz_offset_secs }: Props) {
  const [nextRuns, setNextRuns] = useState<string[]>([]);
  const [cronError, setCronError] = useState<string | null>(null);

  // Cron이 변경될 때 다음 실행 시간 조회
  useEffect(() => {
    if (cron.trim() === "") {
      setNextRuns([]);
      setCronError(null);
      return;
    }
    setCronError(null);
    cronNextRuns(cron, tz_offset_secs, 5)
      .then(setNextRuns)
      .catch((e) => {
        setNextRuns([]);
        setCronError(String(e));
      });
  }, [cron, tz_offset_secs]);

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        {TEMPLATES.map(({ label, cron: template }) => (
          <button
            key={template}
            onClick={() => onCronChange(template)}
            className="h-8 px-3 rounded-md bg-raised text-text-secondary text-sm hover:text-text hover:bg-bg transition"
          >
            {label}
          </button>
        ))}
      </div>

      <div className="flex gap-2 items-center">
        <input
          className={`${inputCls} flex-1 min-w-64`}
          placeholder="초 분 시 일 월 요일 (예: 0 0 9 * * *)"
          value={cron}
          onChange={(e) => onCronChange(e.target.value)}
        />
      </div>

      {cronError && <div className="text-status-failed text-xs font-code">{cronError}</div>}

      {nextRuns.length > 0 && (
        <div className="bg-bg border border-border rounded p-2">
          <div className="text-xs text-text-muted mb-1">다음 실행 시점</div>
          <div className="text-xs space-y-1 font-code text-text-secondary">
            {nextRuns.map((run, i) => (
              <div key={i}>• {run}</div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
