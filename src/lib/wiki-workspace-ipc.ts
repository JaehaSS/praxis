import { invoke } from "@tauri-apps/api/core";
import { LOCAL_HOST, type HostId } from "./transport";

export interface WikiPage {
  id: string; path: string; title: string; aliases: string[]; tags: string[];
  /** 프런트매터의 분류. 값이 없는 문서는 하네스가 `page`로 채운다. */
  type: string; status: string; scope: string;
  body: string; source_prefix: string; outgoing: string[]; backlinks: string[]; sha256: string;
}
export interface WikiEdge {
  source: string; target: string;
  evidence: { line: number; target: string; syntax: string; anchor: string }[];
}
export interface WikiGraph {
  schema_version: number; nodes: WikiPage[]; edges: WikiEdge[];
  diagnostics: { kind: string; source: string; target: string }[]; writable: boolean;
}
export interface WikiFile { path: string; content: string; sha256: string; }
function local(host: HostId) {
  if (host !== LOCAL_HOST) throw new Error("위키 파일은 로컬 세션에서만 사용할 수 있습니다");
}
export function wikiGraph(vaultId: string, host: HostId) {
  local(host); return invoke<WikiGraph>("wiki_workspace_graph", { vaultId });
}
export function wikiRead(vaultId: string, path: string, host: HostId) {
  local(host); return invoke<WikiFile>("wiki_workspace_read", { vaultId, path });
}
export function wikiSave(vaultId: string, path: string, content: string, expectedHash: string | null, host: HostId) {
  local(host); return invoke<WikiFile>("wiki_workspace_save", { vaultId, path, content, expectedHash });
}
export function wikiTrash(vaultId: string, path: string, expectedHash: string, host: HostId) {
  local(host); return invoke<void>("wiki_workspace_trash", { vaultId, path, expectedHash });
}
