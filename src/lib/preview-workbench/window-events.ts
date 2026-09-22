import { invoke } from "@tauri-apps/api/core";
import type { ThemePayload } from "../theme-sync";
import type { PreviewWorkbenchState } from "./types";

export const PREVIEW_TOOLBAR_EVENT = "preview-workbench://toolbar-state";
export const PREVIEW_TOOLBAR_REQUEST = "preview-workbench://toolbar-request";
export const PREVIEW_TOOLBAR_ENTRY = "preview-toolbar";
export type ToolbarAction = "ask" | "cancel" | "take_over" | "release" | "draft" | "resize";

export interface ToolbarEnvelope {
  kind: "ready" | "intent" | "state" | "ack" | "error";
  appEpoch: string;
  taskId: number;
  toolbarLabel: string;
  windowGeneration: number;
  correlationId: string;
}

export interface ToolbarIntent extends ToolbarEnvelope {
  kind: "intent";
  action: ToolbarAction;
  text?: string;
  targetCorrelationId?: string;
  requestId?: string;
  height?: number;
}
export interface ToolbarReady extends ToolbarEnvelope { kind: "ready"; }

export interface ToolbarState extends ToolbarEnvelope {
  kind: "state";
  revision: number;
  state: PreviewWorkbenchState;
  theme: ThemePayload;
}

export type ToolbarAck = ToolbarEnvelope & ({ kind: "ack"; status: "queued"; } | { kind: "ack"; status: "accepted" | "finished"; requestId: string; });

export interface ToolbarError extends ToolbarEnvelope { kind: "error"; error: string; requestId?: string; }
export type ToolbarMessage = ToolbarReady | ToolbarIntent | ToolbarState | ToolbarAck | ToolbarError;
export interface ToolbarIdentity { appEpoch: string; taskId: number; toolbarLabel: string; windowGeneration: number; }

export function isPreviewToolbarEntry(search: string): boolean {
  return new URLSearchParams(search).get("window") === PREVIEW_TOOLBAR_ENTRY;
}

export function toolbarHeight(state: PreviewWorkbenchState): number {
  if (state.error) return 160;
  if (state.pending || state.busy !== "idle" || state.takenOver) return 128;
  return 96;
}

export function matchesToolbarIdentity(message: ToolbarEnvelope, identity: ToolbarIdentity): boolean {
  return message.appEpoch === identity.appEpoch
    && message.taskId === identity.taskId
    && message.toolbarLabel === identity.toolbarLabel
    && message.windowGeneration === identity.windowGeneration;
}

export function relayToolbarMessage(message: ToolbarReady | ToolbarIntent): Promise<void> {
  return invoke("plugin:preview-workbench|relay", { message });
}

export function publishToolbarMessage(message: ToolbarState | ToolbarAck | ToolbarError): Promise<void> {
  return invoke("plugin:preview-workbench|publish", { message });
}
