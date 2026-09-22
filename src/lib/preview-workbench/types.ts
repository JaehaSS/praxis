export type PreviewBusy = "unknown" | "busy" | "idle";
export type PreviewSource = "manual" | "preview_queue";
export type PreviewReceiptStatus =
  | "prepared"
  | "accepted"
  | "finished"
  | "rejected"
  | "retired"
  | "invalidated";

export interface PreviewWorkbenchRemoteState {
  taskId: number;
  appEpoch: string;
  busy: Exclude<PreviewBusy, "unknown">;
  url: string | null;
  convoActive: boolean;
  takenOver: boolean;
  supported: boolean;
  unsupportedReason: string | null;
}

export interface PreviewReceipt {
  requestId: string;
  status: PreviewReceiptStatus;
  accepted: boolean;
  running: boolean;
  retryable: boolean;
  resultId?: string;
}

export interface PreviewPending {
  message: string;
  correlationId: string;
  source: PreviewSource;
}

export interface PreviewFlight extends PreviewPending {
  requestId?: string;
  url?: string;
}

export interface PreviewReceiptOutcome {
  correlationId: string;
  status: "queued" | "accepted" | "finished";
  requestId?: string;
}

export interface PreviewWorkbenchState extends Omit<PreviewWorkbenchRemoteState, "busy"> {
  key: string;
  busy: PreviewBusy;
  draft: string;
  displayUrl: string | null;
  pending: PreviewPending | null;
  inFlight: PreviewFlight | null;
  error: string | null;
  lastAction: string | null;
  receipt?: PreviewReceiptOutcome | null;
  revision: number;
}
