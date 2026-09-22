import { invoke } from "@tauri-apps/api/core";

import { LOCAL_HOST, type HostId } from "./transport";

export type VaultScope = "private-data" | "common" | "project";

export interface Vault { id: string; vault_root: string; enabled: boolean; }
export interface VaultBinding { id: string; epoch: string; canonical_repo_root: string; }
export interface VaultDocument { id: string; vault_id: string; kind: "source" | "note" | "url"; title: string; state: "active" | "archived" | "missing" | "drifted"; current_revision_id: string | null; current_revision_hash?: string | null; current_scope: VaultScope; }
export interface VaultRevision { id: string; document_id: string; relative_path: string; sha256: string; size: number; predecessor: string | null; }
export interface VaultRelatedDocument { document_id: string; title: string; revision_id: string; }
export interface VaultDocumentDetail { document: VaultDocument; current_revision: VaultRevision | null; revision_history: VaultRevision[]; bounded_text: string | null; read_error: string | null; unsupported_reason: string | null; source_revisions: VaultRelatedDocument[]; backlinks: VaultRelatedDocument[]; index_status: "indexed" | "not-indexed"; index_reason: string | null; }
export interface VaultReference { document_id: string; title: string; scope: VaultScope; revision_id: string; revision_hash: string; snippet: string; reason: string; excluded: boolean; stale_reason: string | null; }
export interface VaultSearchPage { hits: VaultReference[]; has_more: boolean; }
export interface VaultPreview { id: string; query_hash: string; created_at: number; references: VaultReference[]; }
export interface VaultUsage { attempt_id: string; revision_id: string; revision_hash: string; snippet: string; snippet_hash: string; delivery_state: "pending" | "delivered" | "not_delivered"; citation_state: "cited" | "unknown"; }
export interface VaultCaptureConsent { id: string; provider: string; }
export interface VaultOperationConflict { id: string; vault_id: string; document_id: string; revision_id: string; relative_path: string; }
export interface VaultRecovery { recovered: number; conflicts: number; reindex_needed: number; }
export interface VaultBindingHistory extends VaultBinding { active: boolean; }
export interface VaultStatus { supported: boolean; vaults: Vault[]; index_rebuild_needed: boolean; provider_available: boolean; current_provider: string; current_provider_display: string; project_binding: VaultBinding | null; current_consent: VaultCaptureConsent | null; prior_bindings: VaultBindingHistory[]; operation_conflicts: VaultOperationConflict[]; }
export interface VaultImportedSource { document_id: string; revision_id: string; sha256: string; }
export interface VaultImportPathResult { path: string; source: VaultImportedSource | null; error: string | null; }
export interface VaultScan { indexed: number; skipped: number; partial: boolean; warnings: string[]; }
/** 정리 세션 결과 — 창 label·루트와 스킬 설치 안내(차단이 아니다). `skill`은 실제로 부른 스킬 이름이다. */
export interface VaultSessionOpen { label: string; root: string; skill: string; skill_present: boolean; warning: string | null; }
/** 창고 폴더 설정 — `wiki_dir`은 창고 루트 기준 상대 경로, `organizer_skill`은 정리 세션이 부르는 스킬,
 *  `wiki_home`은 위키를 열 때 먼저 띄울 진입 문서(경로 또는 파일 이름). */
export interface VaultFolderSettings { wiki_dir: string; organizer_skill: string; wiki_home: string; }
export type VaultDraftInputMode = "default" | "task_only" | "private_attachment";

function local(host: HostId) {
  if (host !== LOCAL_HOST) throw new Error("개인 지식창고는 로컬 세션에서만 사용할 수 있습니다");
}

export const vaultStatus = (host: HostId = LOCAL_HOST, repoRoot?: string) => { local(host); return invoke<VaultStatus>("knowledge_vault_status", { repoRoot }); };
export const vaultConnect = (vaultRoot: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<Vault>("knowledge_vault_connect", { vaultRoot }); };
export const vaultDisconnect = (vaultId: string, host: HostId = LOCAL_HOST) => { local(host); return invoke("knowledge_vault_disconnect", { vaultId }); };
export const vaultSessionOpen = (vaultId: string, agent: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultSessionOpen>("knowledge_vault_session_open", { vaultId, agent }); };
export const vaultSettingsGet = (host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultFolderSettings>("knowledge_vault_settings_get"); };
export const vaultSettingsSet = (wikiDir: string, organizerSkill: string, wikiHome: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultFolderSettings>("knowledge_vault_settings_set", { wikiDir, organizerSkill, wikiHome }); };
export const vaultDocuments = (vaultId: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultDocument[]>("knowledge_vault_list_documents", { vaultId }); };
export const vaultDocument = (documentId: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultDocumentDetail>("knowledge_vault_document", { documentId }); };
export const vaultArchive = (documentId: string, archived: boolean, host: HostId = LOCAL_HOST) => { local(host); return invoke("knowledge_vault_archive", { documentId, archived }); };
export const vaultScope = (revisionId: string, scope: VaultScope, repoRoot?: string, host: HostId = LOCAL_HOST) => { local(host); return invoke("knowledge_vault_scope", { revisionId, scope, repoRoot }); };
export const vaultCreateNote = (vaultId: string, title: string, body: string, sourceRevisions: string[], scope: VaultScope, repoRoot?: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultRevision>("knowledge_vault_create_note", { vaultId, title, body, sourceRevisions, scope, repoRoot }); };
export const vaultUpdateNote = (vaultId: string, documentId: string, expectedBase: string, body: string, sourceRevisions: string[], scope: VaultScope, repoRoot?: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultRevision>("knowledge_vault_update_note", { vaultId, documentId, expectedBase, body, sourceRevisions, scope, repoRoot }); };
export const vaultSearch = (query: string, offset = 0, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultSearchPage>("knowledge_vault_search", { query, offset }); };
export const vaultPreview = (repoRoot: string, query: string, clientRef: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultPreview>("knowledge_vault_preview", { repoRoot, query, clientRef }); };
export const vaultExcludePreviewReference = (previewId: string, revisionId: string, host: HostId = LOCAL_HOST) => { local(host); return invoke("knowledge_vault_preview_exclude", { previewId, revisionId }); };
export const vaultRegisterProject = (repoRoot: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultBinding>("knowledge_vault_register_project", { repoRoot }); };
export const vaultRebindProject = (bindingId: string, repoRoot: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultBinding>("knowledge_vault_rebind_project", { bindingId, repoRoot }); };
export const vaultRebind = (vaultId: string, newRoot: string, confirmedRevisions: string[], host: HostId = LOCAL_HOST) => { local(host); return invoke<Vault>("knowledge_vault_rebind", { vaultId, newRoot, confirmedRevisions }); };
export const vaultUsage = (taskId: number, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultUsage[]>("knowledge_vault_usage", { taskId }); };
export const vaultImportFiles = (vaultId: string, paths: string[], scope: VaultScope = "private-data", repoRoot?: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultImportPathResult[]>("knowledge_vault_import_files", { vaultId, paths, scope, repoRoot }); };
export const vaultCreateTextSource = (vaultId: string, title: string, body: string, scope: VaultScope = "private-data", repoRoot?: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultImportedSource>("knowledge_vault_text_source", { vaultId, title, body, scope, repoRoot }); };
export const vaultCreateUrlSource = (vaultId: string, title: string, url: string, memo: string, scope: VaultScope = "private-data", repoRoot?: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultImportedSource>("knowledge_vault_url_source", { vaultId, title, url, memo, scope, repoRoot }); };
export const vaultScan = (vaultId: string, exclusions: string[], scope: VaultScope = "private-data", repoRoot?: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultScan>("knowledge_vault_scan", { vaultId, exclusions, scope, repoRoot }); };
export const vaultOpenOriginal = (revisionId: string, host: HostId = LOCAL_HOST) => { local(host); return invoke<string>("knowledge_vault_open_original", { revisionId }); };
export const vaultRecoverOperations = (host: HostId = LOCAL_HOST) => { local(host); return invoke<VaultRecovery>("knowledge_vault_recover_operations"); };
export const vaultSetDraftPolicy = (repoRoot: string, query: string, clientRef: string, inputMode: VaultDraftInputMode, revisionIds: string[], host: HostId = LOCAL_HOST) => { local(host); return invoke("knowledge_vault_draft_policy_set", { repoRoot, query, clientRef, inputMode, revisionIds }); };
export const vaultLocalComposerSend = (taskId: number, message: string, expectedClientRef: string, host: HostId = LOCAL_HOST) => { local(host); return invoke("knowledge_vault_local_composer_send", { id: taskId, message, expectedClientRef }); };
