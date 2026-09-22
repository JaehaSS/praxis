import { invoke } from "@tauri-apps/api/core";

export interface WikiSpace {
  id: string;
  name: string;
  root: string;
}

export interface WikiDocumentSummary {
  node_id: number;
  space_id: string;
  relative_path: string;
  title: string;
  snippet: string;
}

export interface WikiDocument {
  node_id: number;
  space_id: string;
  relative_path: string;
  title: string;
  path: string;
  body: string;
}

export interface WikiSyncResult {
  indexed: number;
  skipped: number;
  deleted: number;
  edges: number;
  complete: boolean;
  warnings: string[];
}

export const wikiSpaces = () => invoke<WikiSpace[]>("wiki_spaces");

export const wikiConnect = (root: string) => invoke<WikiSpace>("wiki_connect", { root });

export const wikiSync = () => invoke<WikiSyncResult>("wiki_sync");

export const wikiDocuments = (spaceId?: string, query = "") =>
  invoke<{ documents: WikiDocumentSummary[]; truncated: boolean }>("wiki_documents", { spaceId, query });

export const wikiReadDocument = (nodeId: number) =>
  invoke<WikiDocument>("wiki_read_document", { nodeId });
