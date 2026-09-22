import { invoke } from "@tauri-apps/api/core";

import type { VaultSessionOpen } from "./knowledge-vault-ipc";
import { LOCAL_HOST, type HostId } from "./transport";

/** 메모리 루트와 상한. `root`는 사용자가 지정한 값(빈 문자열이면 기본), `effective_root`는 실제로 쓰이는 절대 경로. */
export interface MemorySettings { root: string; effective_root: string; cap_lines: number; cap_bytes: number; user_cap_lines: number; user_cap_bytes: number; }

/** 메모리 파일 한 줄 — 본문은 담지 않는다. 정본은 파일이고 여기 있는 것은 크기·시각뿐이다(설계 R8). */
export interface MemoryFileInfo { path: string; kind: "user" | "repo"; repo: string | null; repo_key: string | null; exists: boolean; lines: number; bytes: number; cap_lines: number; cap_bytes: number; modified_at: number | null; last_projected_at: number | null; last_task_id: number | null; }

function local(host: HostId) {
  if (host !== LOCAL_HOST) throw new Error("메모리 파일은 로컬 세션에서만 사용할 수 있습니다");
}

export const memorySettingsGet = (host: HostId = LOCAL_HOST) => { local(host); return invoke<MemorySettings>("memory_settings_get"); };
export const memorySettingsSet = (settings: MemorySettings, host: HostId = LOCAL_HOST) => { local(host); return invoke<MemorySettings>("memory_settings_set", { settings }); };
export const memoryFilesList = (host: HostId = LOCAL_HOST) => { local(host); return invoke<MemoryFileInfo[]>("memory_files_list"); };
export const memoryFileOpen = (path: string, host: HostId = LOCAL_HOST) => { local(host); return invoke("memory_file_open", { path }); };
export const memorySessionOpen = (agent: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultSessionOpen>("memory_session_open", { agent }); };
