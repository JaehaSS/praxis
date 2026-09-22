import { invoke } from "@tauri-apps/api/core";

export type NotificationKind = "result" | "question" | "failure";

export interface ResultNotice {
  sequence: number;
  task_id: number;
  ts: number;
  kind: NotificationKind;
  title: string;
  repo: string;
}

export interface SourcePage {
  source_id: string;
  after: number | null;
  cursor: number;
  watermark: number;
  results: ResultNotice[];
}

export interface InboxItem extends ResultNotice {
  host: string;
  source_id: string;
  read_sequence: number;
}

export interface SourceCursor {
  host: string;
  source_id: string;
  cursor: number;
  warning: string | null;
}

export interface NotificationSnapshot {
  items: InboxItem[];
  sources: SourceCursor[];
  enabled: boolean;
  delivery_error: string | null;
}

export const notificationSourcePage = (after: number | null) =>
  invoke<SourcePage>("notification_source_page", { after });
export const notificationSnapshot = () => invoke<NotificationSnapshot>("notification_snapshot");
export const notificationIngest = (host: string, page: SourcePage, notify: boolean) =>
  invoke<NotificationSnapshot>("notification_ingest", { host, page, notify });
export const notificationAcknowledge = (host: string, sourceId: string, taskId: number, throughSequence: number) =>
  invoke<NotificationSnapshot>("notification_acknowledge", { host, sourceId, taskId, throughSequence });
export const notificationReconcile = (host: string, taskIds: number[]) =>
  invoke<NotificationSnapshot>("notification_reconcile", { host, taskIds });
export const notificationSettingsSet = (enabled: boolean) =>
  invoke<NotificationSnapshot>("notification_settings_set", { enabled });
export const notificationTest = () => invoke<void>("notification_test");
export const notificationPermission = () => invoke<string>("notification_permission");
