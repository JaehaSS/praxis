import type { OutcomeInsights } from "../../lib/ipc";

interface MetricDescription {
  value: string;
  detail: string;
}

export function describeNoReexplanation(data: OutcomeInsights): MetricDescription {
  const coverage = `관측 ${data.no_reexplanation_observed_task_count} / ${data.no_reexplanation_target_task_count}`;
  const proxy = "초기 지시 후 추가 입력 0회의 보수적 proxy";
  if (data.no_reexplanation_completion_rate == null) {
    const value = data.no_reexplanation_target_task_count === 0 ? "계측 전 · N/A" : "N/A";
    return {
      value,
      detail: `${coverage} · 미계측 ${data.no_reexplanation_unmeasured_task_count} · ${proxy}`,
    };
  }
  return {
    value: `${Math.round(data.no_reexplanation_completion_rate * 100)}% · ${data.no_reexplanation_success_task_count} / ${data.no_reexplanation_observed_task_count}`,
    detail: `${coverage} · ${proxy}`,
  };
}
