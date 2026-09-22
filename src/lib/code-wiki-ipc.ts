import { invoke } from "@tauri-apps/api/core";

export type CodeWikiPageState = "missing" | "ready" | "stale" | "conflict" | "orphaned";

export interface CodeWikiModule {
  sourcePath: string;
  pagePath: string;
  state: CodeWikiPageState;
}

export interface CodeWikiStatus {
  graphState: string;
  indexPath: string;
  indexState: CodeWikiPageState;
  modules: CodeWikiModule[];
  detail: string | null;
}

export const codewikiStatus = (id: number) =>
  invoke<CodeWikiStatus>("codewiki_status", { id });

export const codewikiGenerate = (id: number, sourcePath: string | null = null) =>
  invoke<CodeWikiStatus>("codewiki_generate", { id, sourcePath });
