import type { ReactElement } from "react";
import type { OutcomeInsights } from "../../lib/ipc";
import {
  deriveOutcomeOpportunities,
  type OutcomeOpportunity,
  type OutcomeOpportunityAction,
} from "./outcome-opportunities";

interface Props {
  data: OutcomeInsights;
  onOpenMemory?: () => void;
}

export function OutcomeGoalList({ data, onOpenMemory }: Props): ReactElement {
  const opportunities = deriveOutcomeOpportunities(data);
  if (opportunities.length === 0) {
    return <p className="text-xs text-text-muted">현재 집계에서 즉시 드러난 gap이 없습니다.</p>;
  }
  return (
    <section aria-label="추천 goal" className="rounded-md border border-border bg-raised/30 p-3">
      <h3 className="text-xs font-medium uppercase tracking-wide text-text-secondary">추천 goal</h3>
      <div className="mt-2 grid gap-2 sm:grid-cols-2">
        {opportunities.map((opportunity) => (
          <OutcomeGoal
            key={opportunity.id}
            opportunity={opportunity}
            onAction={resolveAction(opportunity.action, onOpenMemory)}
          />
        ))}
      </div>
    </section>
  );
}

function OutcomeGoal({
  opportunity,
  onAction,
}: {
  opportunity: OutcomeOpportunity;
  onAction?: () => void;
}): ReactElement {
  return (
    <article className="rounded border border-border bg-surface p-2.5">
      <div className="text-xs font-medium text-text">{opportunity.title}</div>
      <p className="mt-1 text-xs leading-relaxed text-text-muted">{opportunity.reason}</p>
      {opportunity.actionLabel && onAction && (
        <button
          type="button"
          className="mt-2 rounded-md bg-primary/15 px-2.5 py-1 text-xs text-primary-bright hover:bg-primary/20"
          onClick={onAction}
        >
          {opportunity.actionLabel}
        </button>
      )}
    </article>
  );
}

function resolveAction(
  action: OutcomeOpportunityAction,
  onOpenMemory?: () => void,
): (() => void) | undefined {
  if (action === "open_memory") return onOpenMemory;
  return undefined;
}
