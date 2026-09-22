import { invoke } from "@tauri-apps/api/core";
import type { FileContent, FsNode } from "./ipc";

export interface ProjectEditorInfo {
  root: string;
  label: string;
  /** 창의 첫 셸이 에이전트 CLI로 뜬다 — 창이 터미널을 처음부터 연다. */
  launch?: boolean;
}

export interface ProjectShellOpen {
  session: number;
  existed: boolean;
}

export interface ProjectShellSnapshot {
  session: number;
  sequence: number;
  data: string;
  exited: boolean;
  exit_code: number | null;
}

export interface ProjectShellOutput {
  session: number;
  sequence: number;
  data: string;
}

export interface ProjectShellExit {
  session: number;
  code: number;
}

export const projectEditorOpen = (root: string) =>
  invoke<ProjectEditorInfo>("project_editor_open", { root });
export const projectEditorInfo = () => invoke<ProjectEditorInfo>("project_editor_info");
export const projectEditorTree = () => invoke<FsNode[]>("project_editor_tree");
export const projectEditorRead = (path: string) =>
  invoke<FileContent>("project_editor_read", { path });
export const projectEditorWrite = (path: string, content: string, expectedContent: string) =>
  invoke<number>("project_editor_write", { path, content, expectedContent });
export const projectEditorResolvePath = (path: string) =>
  invoke<string>("project_editor_resolve_path", { path });
export const projectEditorOpenPath = (path: string) =>
  invoke<void>("project_editor_open_path", { path });
export const projectShellOpen = (cols: number, rows: number) =>
  invoke<ProjectShellOpen>("project_editor_shell_open", { cols, rows });
export const projectShellSnapshot = (session: number) =>
  invoke<ProjectShellSnapshot>("project_editor_shell_snapshot", { session });
export const projectShellWrite = (session: number, data: string) =>
  invoke<void>("project_editor_shell_write", { session, data });
export const projectShellResize = (session: number, cols: number, rows: number) =>
  invoke<void>("project_editor_shell_resize", { session, cols, rows });
export const projectShellClose = (session: number) =>
  invoke<void>("project_editor_shell_close", { session });
