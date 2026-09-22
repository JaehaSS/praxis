import type { WorkflowSnapshot } from "./api";

export interface WorkflowEvent {
  sequence: number;
  kind: string;
  detail?: string | null;
}

/** Server snapshots win. Events only advance a cursor; they never manufacture lifecycle completion. */
export function reconcileWorkflowEvents(
  snapshot: WorkflowSnapshot | null,
  events: readonly WorkflowEvent[],
): { snapshot: WorkflowSnapshot | null; cursor: number } {
  let cursor = snapshot?.last_sequence ?? 0;
  for (const event of events) if (Number.isInteger(event.sequence) && event.sequence > cursor) cursor = event.sequence;
  return { snapshot, cursor };
}

export function pollingDelay(visible: boolean, state: WorkflowSnapshot["state"] | undefined): number {
  return visible && (state === "running" || state === "paused") ? 1_000 : 5_000;
}
