import { useEffect, useRef, useState } from "react";

import {
  vaultExcludePreviewReference,
  vaultDocuments,
  vaultPreview,
  vaultSetDraftPolicy,
  vaultStatus,
  vaultUsage,
  type VaultDraftInputMode,
  type VaultDocument,
  type VaultPreview,
  type VaultUsage,
} from "../../lib/knowledge-vault-ipc";
import { LOCAL_HOST, type HostId } from "../../lib/transport";
import { vaultButton, vaultError } from "./ui";

interface Props {
  host: HostId;
  repo?: string;
  query: string;
  clientRef?: string;
  taskId?: number;
  onVaultMutationPendingChange?: (pending: boolean) => void;
}

export function VaultReferences({ host, repo, query, clientRef, taskId, onVaultMutationPendingChange }: Props) {
  const [preview, setPreview] = useState<VaultPreview | null>(null);
  const [usage, setUsage] = useState<VaultUsage[]>([]);
  const [inputMode, setInputMode] = useState<VaultDraftInputMode>("default");
  const [policyStale, setPolicyStale] = useState(false);
  const [privateDocuments, setPrivateDocuments] = useState<VaultDocument[]>([]);
  const [privateAttachmentsOpen, setPrivateAttachmentsOpen] = useState(false);
  const [privateAttachments, setPrivateAttachments] = useState<Record<string, string>>({});
  const [privateQuery, setPrivateQuery] = useState("");
  const [privatePage, setPrivatePage] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [usageRefresh, setUsageRefresh] = useState(0);
  const previewRequest = useRef(0);
  const policyRequest = useRef(0);
  const attachmentRequest = useRef(0);
  const inputModeRef = useRef<VaultDraftInputMode>("default");
  const contextRef = useRef({ host, repo, clientRef, taskId });
  const usageRequest = useRef(0);
  const local = host === LOCAL_HOST;
  const ready = local && Boolean(repo?.trim() && query.trim() && clientRef);

  useEffect(() => {
    onVaultMutationPendingChange?.(busy || policyStale);
  }, [busy, onVaultMutationPendingChange, policyStale]);

  useEffect(() => () => onVaultMutationPendingChange?.(false), [onVaultMutationPendingChange]);

  useEffect(() => {
    const previous = contextRef.current;
    const contextChanged = previous.host !== host || previous.repo !== repo || previous.clientRef !== clientRef || previous.taskId !== taskId;
    contextRef.current = { host, repo, clientRef, taskId };
    previewRequest.current += 1;
    policyRequest.current += 1;
    attachmentRequest.current += 1;
    setPreview(null);
    setPrivateAttachmentsOpen(false);
    if (contextChanged) {
      inputModeRef.current = "default";
      setInputMode("default");
      setPrivateAttachments({});
      setPrivateDocuments([]);
      setPolicyStale(false);
    } else {
      setPolicyStale(inputModeRef.current !== "default");
    }
    setError("");
    setBusy(false);
  }, [host, repo, query, clientRef, taskId]);

  useEffect(() => {
    const current = ++usageRequest.current;
    if (!local || taskId == null) {
      setUsage([]);
      return;
    }
    void vaultUsage(taskId, host).then((next) => {
      if (current === usageRequest.current) setUsage(next);
    }).catch((reason) => {
      if (current === usageRequest.current) setError(String(reason));
    });
  }, [host, local, taskId, usageRefresh]);

  if (!local) return null;

  const load = async () => {
    if (!ready || busy || !repo || !clientRef) return;
    const current = ++previewRequest.current;
    setBusy(true);
    setError("");
    try {
      const next = await vaultPreview(repo, query.trim(), clientRef, host);
      if (current === previewRequest.current) setPreview(next);
    } catch (reason) {
      if (current === previewRequest.current) setError(String(reason));
    } finally {
      if (current === previewRequest.current) setBusy(false);
    }
  };

  const exclude = async (revisionId: string) => {
    if (!preview || busy) return;
    const previewId = preview.id;
    const current = ++previewRequest.current;
    setBusy(true);
    setError("");
    try {
      await vaultExcludePreviewReference(previewId, revisionId, host);
      if (current !== previewRequest.current) return;
      setPreview((value) => value?.id === previewId
        ? { ...value, references: value.references.map((reference) => (
          reference.revision_id === revisionId ? { ...reference, excluded: true } : reference
        )) }
        : value);
    } catch (reason) {
      if (current === previewRequest.current) setError(String(reason));
    } finally {
      if (current === previewRequest.current) setBusy(false);
    }
  };

  const setPolicy = async (next: VaultDraftInputMode) => {
    const saved = await savePolicy(next, []);
    if (saved && next === "default") setPrivateAttachments({});
  };

  const savePolicy = async (next: VaultDraftInputMode, revisionIds: string[]) => {
    if (!ready || busy || !repo || !clientRef) return false;
    const current = ++policyRequest.current;
    setBusy(true);
    setError("");
    try {
      await vaultSetDraftPolicy(repo, query.trim(), clientRef, next, revisionIds, host);
      if (current !== policyRequest.current) return false;
      inputModeRef.current = next;
      setInputMode(next);
      setPolicyStale(false);
      return true;
    } catch (reason) {
      if (current === policyRequest.current) setError(String(reason));
      return false;
    } finally {
      if (current === policyRequest.current) setBusy(false);
    }
  };

  const openPrivateAttachments = async () => {
    if (busy || privateAttachmentsOpen) return;
    const current = ++attachmentRequest.current;
    setBusy(true);
    setError("");
    try {
      const status = await vaultStatus(host);
      const vault = status.vaults.find((item) => item.enabled);
      if (!vault) throw new Error("연결된 자료함이 없습니다");
      const documents = await vaultDocuments(vault.id, host);
      if (current !== attachmentRequest.current) return;
      setPrivateDocuments(documents.filter((item) => item.state === "active" && item.current_scope === "private-data" && item.current_revision_id));
      setPrivateAttachmentsOpen(true);
    } catch (reason) {
      if (current === attachmentRequest.current) setError(String(reason));
    } finally {
      if (current === attachmentRequest.current) setBusy(false);
    }
  };

  const togglePrivateAttachment = async (document: VaultDocument) => {
    if (!document.current_revision_id || busy) return;
    const current = privateAttachments[document.id];
    const next = { ...privateAttachments };
    if (current) delete next[document.id];
    else {
      if (Object.keys(next).length === 5) return;
      if (!document.current_revision_hash) {
        setError("선택한 자료의 현재 버전을 확인할 수 없습니다");
        return;
      }
      setBusy(true);
      next[document.id] = document.current_revision_hash;
      setBusy(false);
    }
    const saved = await savePolicy(Object.keys(next).length ? "private_attachment" : "task_only", Object.entries(next).map(([documentId]) => privateDocuments.find((item) => item.id === documentId)?.current_revision_id).filter((item): item is string => Boolean(item)));
    if (saved) setPrivateAttachments(next);
  };

  const matchingPrivateDocuments = privateDocuments.filter((item) => item.title.toLocaleLowerCase().includes(privateQuery.toLocaleLowerCase()));
  const visiblePrivateDocuments = matchingPrivateDocuments.slice(privatePage * 100, (privatePage + 1) * 100);

  return <section aria-label="작업 참고 자료" className="mb-1 min-w-0 rounded-md border border-border bg-surface px-2 py-1 text-sm">
    <details>
    <summary className="cursor-pointer rounded text-xs font-medium leading-5 text-text-secondary hover:text-text focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"><h2 className="inline">작업 참고 자료</h2>{policyStale && <span className="ml-2 text-xs text-status-failed">확인 필요</span>}</summary>
    <div className="mt-2 flex justify-end"><button className={vaultButton} type="button" disabled={!ready || busy} onClick={() => void load()}>{busy ? "불러오는 중…" : "관련 자료 미리보기"}</button></div>
    <p className="mt-1 text-xs text-text-secondary">참고자료는 이번 작업에만 전달됩니다. 참고자료가 포함된 회차는 자동 정리에서 제외됩니다.</p>
    <div className="mt-2 flex flex-wrap items-center gap-2 text-xs"><span>입력 처리:</span><button className={vaultButton} type="button" disabled={!ready || busy || (inputMode === "default" && !policyStale)} onClick={() => void setPolicy("default")}>기본</button><button className={vaultButton} type="button" disabled={!ready || busy || (inputMode === "task_only" && !policyStale)} onClick={() => void setPolicy("task_only")}>이번 작업만</button><button className={vaultButton} type="button" disabled={!ready || busy} onClick={() => void openPrivateAttachments()}>나만 보기 자료 첨부</button>{policyStale && <span className="break-words text-status-failed">입력이 바뀌어 다시 확인이 필요합니다.</span>}</div>
    {privateAttachmentsOpen && <div className="mt-2 space-y-1 border-t border-border pt-2 text-xs"><label>나만 보기 자료 검색<input className="ml-2 rounded border border-border bg-bg px-2 py-1" value={privateQuery} onChange={(event) => { setPrivateQuery(event.target.value); setPrivatePage(0); }} /></label>{visiblePrivateDocuments.map((document) => <div className="flex items-center justify-between gap-2" key={document.id}><span className="min-w-0 break-all">{document.title} · {privateAttachments[document.id] ?? document.current_revision_id} · 나만 보기</span><button className={vaultButton} type="button" disabled={busy || (!privateAttachments[document.id] && Object.keys(privateAttachments).length === 5)} onClick={() => void togglePrivateAttachment(document)}>{privateAttachments[document.id] ? "첨부 해제" : "첨부"}</button></div>)}{matchingPrivateDocuments.length > 100 && <div className="flex gap-2"><button className={vaultButton} disabled={privatePage === 0} onClick={() => setPrivatePage((value) => value - 1)} type="button">이전</button><button className={vaultButton} disabled={(privatePage + 1) * 100 >= matchingPrivateDocuments.length} onClick={() => setPrivatePage((value) => value + 1)} type="button">다음</button></div>}{privateDocuments.length === 0 && <p className="text-text-secondary">첨부할 나만 보기 자료가 없습니다.</p>}</div>}
    {preview && <p className="mt-2 min-w-0 break-all text-xs text-text-secondary">미리보기 생성: {new Date(preview.created_at * 1000).toLocaleString()}</p>}{preview?.references.map((reference) => <article key={reference.revision_id} className="mt-2 min-w-0 border-t border-border pt-2"><div className="flex flex-wrap items-center justify-between gap-2"><strong className="min-w-0 break-words">{reference.title}</strong><button className={vaultButton} type="button" disabled={busy || reference.excluded} onClick={() => void exclude(reference.revision_id)}>{reference.excluded ? "제외됨" : "제외"}</button></div><p className="min-w-0 break-all text-xs text-text-secondary">{scopeLabel(reference.scope)} · {reference.reason} · {reference.revision_hash}</p><p className="mt-1 break-words">{reference.snippet}</p></article>)}
    {preview && preview.references.length === 0 && <p className="mt-2 text-text-secondary">참고할 자료가 없습니다.</p>}
    {taskId != null && <div className="mt-3 border-t border-border pt-2"><div className="flex flex-wrap items-center justify-between gap-2"><h3 className="font-medium">자료 전달 기록</h3><button className={vaultButton} type="button" onClick={() => setUsageRefresh((value) => value + 1)}>새로 고침</button></div><p className="mt-1 text-xs text-text-secondary">이 기록만 자료가 작업에 전달되었는지를 나타냅니다.</p>{usage.map((item) => <p key={`${item.attempt_id}:${item.revision_id}:${item.snippet_hash}`} className="mt-1 break-all">회차 {item.attempt_id} · 자료 {item.revision_id} · 해시 {item.revision_hash} · {item.snippet} · {deliveryLabel(item.delivery_state)} · {citationLabel(item.citation_state)}</p>)}{usage.length === 0 && <p className="mt-1 text-text-secondary">아직 기록이 없습니다.</p>}</div>}
    </details>
    {error && <p role="alert" className={`${vaultError} mt-2`}>{error}</p>}
  </section>;
}

function scopeLabel(scope: string) { return scope === "private-data" ? "나만 보기" : scope === "common" ? "모든 프로젝트" : "이 프로젝트"; }
function deliveryLabel(state: VaultUsage["delivery_state"]) { return state === "delivered" ? "작업에 전달됨" : state === "not_delivered" ? "전달되지 않음" : "전달 대기"; }
function citationLabel(state: VaultUsage["citation_state"]) { return state === "cited" ? "인용됨" : "인용 여부 알 수 없음"; }
