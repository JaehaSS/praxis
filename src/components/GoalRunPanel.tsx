import { useCallback, useEffect, useState } from "react";
import {
  goalRunCreate,
  goalRunList,
  goalRunStop,
  type GoalBudget,
  type GoalRunView,
} from "../lib/ipc";

/** 0은 무제한이므로 게이지를 그리지 않는다 — 분모가 없는 비율은 그릴 수 없다. */
function isUnlimited(limit: number): boolean {
  return limit <= 0;
}

/**
 * 비용이 0인데 토큰은 썼다면 "안 썼다"가 아니라 **벤더가 비용을 보고하지 않았다**는 뜻이다
 * (codex는 `cost_usd`를 주지 않는다). 0으로 그리면 사용자가 공짜로 읽는다 — #235가
 * "API가 없는 값은 비워 둔다"고 한 것과 같은 자리.
 */
function costUnmeasured(view: GoalRunView): boolean {
  return view.spent.tokens > 0 && view.spent.cost_usd === 0;
}

function Gauge({
  label,
  used,
  limit,
  render,
  testId,
}: {
  label: string;
  used: number;
  limit: number;
  render: (value: number) => string;
  testId: string;
}) {
  if (isUnlimited(limit)) {
    return (
      <div className="flex items-baseline gap-2 text-xs" data-testid={testId}>
        <span className="text-text-muted w-16">{label}</span>
        <span className="font-code">{render(used)}</span>
        <span className="text-text-muted">/ 무제한</span>
      </div>
    );
  }
  const ratio = Math.min(1, used / limit);
  return (
    <div className="flex items-baseline gap-2 text-xs" data-testid={testId}>
      <span className="text-text-muted w-16">{label}</span>
      <span className="font-code">
        {render(used)} / {render(limit)}
      </span>
      <span
        className="flex-1 h-1 bg-border rounded-full overflow-hidden"
        data-testid={`${testId}-bar`}
      >
        <span
          className={`block h-full ${ratio >= 1 ? "bg-status-failed" : "bg-primary"}`}
          style={{ width: `${ratio * 100}%` }}
        />
      </span>
    </div>
  );
}

const STATUS_LABEL: Record<string, string> = {
  running: "진행 중",
  satisfied: "목표 달성",
  exhausted: "예산 소진",
  stopped: "중단됨",
};

function RunCard({
  view,
  onStop,
}: {
  view: GoalRunView;
  onStop: (id: number) => void;
}) {
  const { run, spent } = view;
  const active = run.status === "running";
  return (
    <div className="bg-surface border border-border rounded-lg p-3" data-testid={`run-${run.id}`}>
      <div className="flex items-center gap-2 mb-2">
        <span
          className={`text-xs font-medium ${active ? "text-status-awaiting" : "text-text-muted"}`}
        >
          {STATUS_LABEL[run.status] ?? run.status}
        </span>
        <span className="text-text-muted text-xs font-code truncate">{run.repo}</span>
        {active && (
          <button
            className="ml-auto h-7 px-3 rounded-md text-text-secondary hover:bg-border text-sm"
            onClick={() => onStop(run.id)}
          >
            중단
          </button>
        )}
      </div>
      <div className="text-md mb-2 break-words">{run.goal_contract.objective}</div>
      {run.end_reason && (
        <div className="text-text-muted text-xs mb-2" data-testid={`run-${run.id}-reason`}>
          {run.end_reason}
        </div>
      )}
      <div className="flex flex-col gap-1">
        <Gauge
          label="재진입"
          used={spent.attempts}
          limit={run.budget.max_attempts}
          render={(v) => `${v}회`}
          testId="budget-attempts"
        />
        <Gauge
          label="토큰"
          used={spent.tokens}
          limit={run.budget.max_tokens}
          render={(v) => v.toLocaleString()}
          testId="budget-tokens"
        />
        {costUnmeasured(view) ? (
          <div className="flex items-baseline gap-2 text-xs" data-testid="budget-cost">
            <span className="text-text-muted w-16">비용</span>
            <span className="font-code">—</span>
            <span className="text-text-muted">이 벤더는 비용을 보고하지 않습니다</span>
          </div>
        ) : (
          <Gauge
            label="비용"
            used={spent.cost_usd}
            limit={run.budget.max_cost_usd}
            render={(v) => `$${v.toFixed(2)}`}
            testId="budget-cost"
          />
        )}
        <Gauge
          label="시간"
          used={spent.elapsed_secs}
          limit={run.budget.max_wall_secs}
          render={(v) => `${Math.floor(v / 60)}분`}
          testId="budget-wall"
        />
      </div>
    </div>
  );
}

const DEFAULT_BUDGET: GoalBudget = {
  max_attempts: 3,
  max_tokens: 0,
  max_cost_usd: 0,
  max_wall_secs: 3600,
};

/** 예산이 전부 0이면 정지 조건이 없다 — 백엔드도 거부하므로 버튼을 막아 왕복을 줄인다. */
function budgetIsEmpty(b: GoalBudget): boolean {
  return (
    b.max_attempts <= 0 && b.max_tokens <= 0 && b.max_cost_usd <= 0 && b.max_wall_secs <= 0
  );
}

/** Goal Run — 목표를 예산 안에서 자율 재진입시키고, 소진되면 멈춘다 (계획 0036). */
export function GoalRunPanel({ repo }: { repo?: string }) {
  const [runs, setRuns] = useState<GoalRunView[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [objective, setObjective] = useState("");
  const [budget, setBudget] = useState<GoalBudget>(DEFAULT_BUDGET);

  const refresh = useCallback(async () => {
    try {
      setRuns(await goalRunList(repo));
    } catch (e) {
      setErr(String(e));
    }
  }, [repo]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const stop = async (id: number) => {
    setErr(null);
    try {
      await goalRunStop(id);
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const create = async () => {
    if (!repo) return;
    setErr(null);
    try {
      await goalRunCreate({
        repo,
        agent: "claude",
        instruction: objective,
        goal_contract: {
          schema_version: 1,
          objective,
          acceptance: [],
          stop_conditions: [],
          must_preserve: [],
          protected_paths: [],
          non_goals: [],
        },
        budget,
      });
      setObjective("");
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const canCreate = Boolean(repo) && objective.trim().length > 0 && !budgetIsEmpty(budget);

  const num = (key: keyof GoalBudget) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setBudget({ ...budget, [key]: Number(e.target.value) || 0 });

  return (
    <div className="flex-1 overflow-auto p-4">
      {err && <div className="text-status-failed text-sm font-code mb-2">{err}</div>}
      <div className="max-w-3xl mx-auto">
        <div className="text-text-muted text-xs mb-3">
          목표가 검증을 통과할 때까지 예산 안에서 스스로 다시 시도합니다. 각 시도는 그대로
          승인 대기로 올라오고, 거부하면 Run이 멈춥니다.
        </div>

        <div className="bg-surface border border-border rounded-lg p-3 mb-4">
          <input
            className="w-full bg-bg border border-border rounded-md px-2 h-8 text-sm mb-2"
            placeholder="목표 — 무엇이 되면 끝인가"
            aria-label="목표"
            value={objective}
            onChange={(e) => setObjective(e.target.value)}
          />
          <div className="grid grid-cols-4 gap-2 mb-2">
            <label className="text-xs text-text-muted">
              재진입
              <input
                className="w-full bg-bg border border-border rounded-md px-2 h-7 text-sm"
                type="number"
                aria-label="재진입 상한"
                value={budget.max_attempts}
                onChange={num("max_attempts")}
              />
            </label>
            <label className="text-xs text-text-muted">
              토큰
              <input
                className="w-full bg-bg border border-border rounded-md px-2 h-7 text-sm"
                type="number"
                aria-label="토큰 상한"
                value={budget.max_tokens}
                onChange={num("max_tokens")}
              />
            </label>
            <label className="text-xs text-text-muted">
              비용($)
              <input
                className="w-full bg-bg border border-border rounded-md px-2 h-7 text-sm"
                type="number"
                aria-label="비용 상한"
                value={budget.max_cost_usd}
                onChange={num("max_cost_usd")}
              />
            </label>
            <label className="text-xs text-text-muted">
              시간(초)
              <input
                className="w-full bg-bg border border-border rounded-md px-2 h-7 text-sm"
                type="number"
                aria-label="시간 상한"
                value={budget.max_wall_secs}
                onChange={num("max_wall_secs")}
              />
            </label>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-text-muted text-xs">0은 무제한입니다. 첫 시도는 1분 안에 올라옵니다.</span>
            <button
              className="ml-auto h-8 px-3 rounded-md bg-primary text-bg text-sm font-medium disabled:opacity-40"
              disabled={!canCreate}
              onClick={create}
            >
              Run 시작
            </button>
          </div>
        </div>

        {runs.length === 0 ? (
          <div className="text-text-muted text-center py-12">아직 Run이 없습니다.</div>
        ) : (
          <div className="flex flex-col gap-2">
            {runs.map((view) => (
              <RunCard key={view.run.id} view={view} onStop={stop} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
