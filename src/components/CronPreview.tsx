import { useEffect, useState } from "react";
import { cronNextRuns } from "../lib/ipc";

interface Props {
  cron: string;
  tz_offset_secs: number;
}

export function CronPreview({ cron, tz_offset_secs }: Props) {
  const [nextRuns, setNextRuns] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (cron.trim() === "") {
      setNextRuns([]);
      setError(null);
      return;
    }
    setError(null);
    cronNextRuns(cron, tz_offset_secs, 5)
      .then(setNextRuns)
      .catch((e) => {
        setNextRuns([]);
        setError(String(e));
      });
  }, [cron, tz_offset_secs]);

  if (!cron.trim()) {
    return null;
  }

  if (error) {
    return (
      <div className="mt-2 p-2 rounded bg-status-failed/10 border border-status-failed/30">
        <div className="text-xs text-status-failed font-code">{error}</div>
      </div>
    );
  }

  if (nextRuns.length === 0) {
    return null;
  }

  return (
    <div className="mt-2 p-2 rounded bg-status-done/10 border border-status-done/30">
      <div className="text-xs text-text-muted mb-1">다음 5개 실행 시점</div>
      <div className="text-xs space-y-0.5 font-code text-text-secondary">
        {nextRuns.map((run, i) => (
          <div key={i}>• {run}</div>
        ))}
      </div>
    </div>
  );
}
