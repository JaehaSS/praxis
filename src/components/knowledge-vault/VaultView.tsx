import { WikiWorkspace } from "./WikiWorkspace";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { PROJECT_EDITOR_CLOSED_EVENT } from "../../lib/editor-window-events";
import { LOCAL_HOST, type HostId } from "../../lib/transport";
import { vaultArchive, vaultDocument, vaultDocuments, vaultSearch, vaultScan, vaultSessionOpen, vaultStatus, type VaultDocument, type VaultDocumentDetail } from "../../lib/knowledge-vault-ipc";
import { VaultAddSource } from "./VaultAddSource";
import { VaultDocumentDetail as Detail } from "./VaultDocumentDetail";
import { VaultFolderSettings } from "./VaultFolderSettings";
import { VaultMemoryPanel } from "./VaultMemoryPanel";
import { VaultSettings } from "./VaultSettings";
import { vaultButton, vaultCard, vaultError, vaultInput, vaultPrimaryButton, vaultTab } from "./ui";
import "./VaultLayout.css";

type Tab = "source" | "note" | "memory";

const FILTERS: { tab: Tab; label: string }[] = [{ tab: "note", label: "문서" }, { tab: "source", label: "자료" }, { tab: "memory", label: "메모리" }];

/** 스킬 이름은 설정값이다 — 세션을 연 백엔드가 실제로 부른 이름을 돌려주므로 그것을 그대로 읽는다. */
const skillMissing = (skill: string) => `~/.claude/skills/${skill} 스킬이 없습니다. 세션에서 규칙을 직접 지시하거나 스킬을 먼저 두세요.`;

export function VaultView({ repo, host, agent = "claude", initialTab, taskId = null }: { repo?: string; host: HostId; agent?: string; initialTab?: Tab; taskId?: number | null }) {
  const [docs, setDocs] = useState<VaultDocument[]>([]); const [detail, setDetail] = useState<VaultDocumentDetail | null>(null); const [tab, setTab] = useState<Tab>(initialTab ?? "note"); const [query, setQuery] = useState(""); const [hits, setHits] = useState<string[] | null>(null); const [hasMore, setHasMore] = useState(false); const [page, setPage] = useState(0);
  const [vaultId, setVaultId] = useState(""); const [vaultRoot, setVaultRoot] = useState(""); const [sessionOpening, setSessionOpening] = useState(false); const [loading, setLoading] = useState(true); const [unsupported, setUnsupported] = useState(false); const [error, setError] = useState(""); const [scanMessage, setScanMessage] = useState(""); const [scanning, setScanning] = useState(false); const [management, setManagement] = useState(false); const [documentDirty, setDocumentDirty] = useState(false); const [documentBusy, setDocumentBusy] = useState(false);
  const [fileRefresh, setFileRefresh] = useState(0);
  const loadRequest = useRef(0); const searchRequest = useRef(0); const detailRequest = useRef(0); const scanned = useRef(""); const guard = useRef({ documentDirty: false, documentBusy: false });
  const load = async () => {
    const request = ++loadRequest.current;
    if (host !== LOCAL_HOST) { setLoading(false); setDocs([]); setDetail(null); setVaultId(""); setVaultRoot(""); return; }
    setError("");
    try { const status = await vaultStatus(host); if (request !== loadRequest.current) return; setUnsupported(!status.supported); const vault = status.vaults.find(item => item.enabled); if (!status.supported || !vault) { setVaultId(""); setVaultRoot(""); setDocs([]); setDetail(null); return; } const next = await vaultDocuments(vault.id, host); if (request !== loadRequest.current) return; setVaultId(vault.id); setVaultRoot(vault.vault_root); setDocs(next); setFileRefresh(value => value + 1); }
    catch (reason) { if (request === loadRequest.current) setError(String(reason)); }
    finally { if (request === loadRequest.current) setLoading(false); }
  };
  useEffect(() => { setLoading(true); detailRequest.current++; searchRequest.current++; setPage(0); setHits(null); setDetail(null); void load(); }, [host, repo]);
  useEffect(() => {
    const request = ++searchRequest.current;
    if (host !== LOCAL_HOST || !vaultId || tab !== "source" || !query) { setHits(null); setHasMore(false); return; }
    void vaultSearch(query, page * 100, host).then(result => { if (request === searchRequest.current) { setHits(result.hits.map(hit => hit.document_id)); setHasMore(result.has_more); } }).catch(reason => { if (request === searchRequest.current) setError(String(reason)); });
  }, [host, vaultId, docs, page, query, tab]);
  useEffect(() => { if (!vaultId || scanned.current === vaultId || isBlocked()) return; scanned.current = vaultId; void scan(); }, [vaultId, documentBusy, documentDirty]);
  // 바깥에서 탭을 지정해 들어온 경우(인사이트·설정의 "메모리") 그 탭으로 연다.
  useEffect(() => { if (initialTab) setTab(initialTab); }, [initialTab]);
  useEffect(() => {
    if (host !== LOCAL_HOST || !vaultRoot) return;
    let unlisten: UnlistenFn | undefined; let disposed = false;
    void listen<string>(PROJECT_EDITOR_CLOSED_EVENT, ({ payload }) => { if (payload === vaultRoot) void scan(); }).then(stop => { if (disposed) stop(); else unlisten = stop; });
    return () => { disposed = true; unlisten?.(); };
  }, [host, vaultRoot]);
  const blocked = documentDirty || documentBusy;
  const isBlocked = () => Object.values(guard.current).some(Boolean);
  const blockError = () => setError("수정 또는 저장 중인 내용이 있습니다. 계속 편집하거나 수정 버리기를 선택하세요.");
  const updateDocumentDirty = useCallback((value: boolean) => { guard.current.documentDirty = value; setDocumentDirty(value); }, []);
  const updateDocumentBusy = useCallback((value: boolean) => { guard.current.documentBusy = value; setDocumentBusy(value); }, []);
  const openDocument = async (documentId: string, allowBlocked = false) => {
    if (!allowBlocked && isBlocked()) { blockError(); return; }
    const request = ++detailRequest.current; setError("");
    try { const next = await vaultDocument(documentId, host); if (request !== detailRequest.current) return; setDetail(next); setTab(next.document.kind === "note" ? "note" : "source"); }
    catch (reason) { if (request === detailRequest.current) setError(String(reason)); }
  };
  const scan = async () => { if (isBlocked()) { blockError(); return; } if (scanning || !vaultId) return; setScanning(true); setError(""); try { const result = await vaultScan(vaultId, [], "private-data", repo, host); setScanMessage(`${result.indexed}개 등록 · ${result.skipped}개 건너뜀${result.partial ? " · 일부만 확인했습니다." : ""} ${result.warnings.join(" / ")}`); await load(); } catch (reason) { setScanMessage(String(reason)); } finally { setScanning(false); } };
  const openSession = async () => { setSessionOpening(true); try { const opened = await vaultSessionOpen(vaultId, agent, host); const message = opened.skill_present === false ? skillMissing(opened.skill) : opened.warning; if (message) setScanMessage(message); } catch (reason) { setError(String(reason)); } finally { setSessionOpening(false); } };
  const importSaved = async () => { if (isBlocked()) { blockError(); return; } await load(); setTab("source"); };
  const changeTab = (next: Tab) => { if (isBlocked() && next !== tab) { blockError(); return; } setTab(next); setDetail(null); setPage(0); };
  const closeDetail = () => { if (isBlocked()) { blockError(); return; } setDetail(null); };
  const archive = async () => { if (isBlocked()) { blockError(); return; } if (!detail) return; try { await vaultArchive(detail.document.id, detail.document.state !== "archived", host); await load(); await openDocument(detail.document.id, true); } catch (reason) { setError(String(reason)); } };
  if (host !== LOCAL_HOST) return <section aria-label="개인 지식창고" className={vaultCard}><h1 className="text-lg font-semibold">개인 지식창고</h1><p className="mt-2 text-sm text-text-secondary">개인 지식창고는 로컬 세션에서만 사용할 수 있습니다.</p></section>;
  if (unsupported) return <p className={vaultError}>개인 지식창고는 현재 macOS Desktop에서 지원합니다.</p>;
  const shown = docs.filter(doc => (tab === "source" ? doc.kind !== "note" : doc.kind === "note") && (!hits || hits.includes(doc.id)));
  return <section className="vault-root min-w-0 max-w-[70rem] space-y-4"><header className={`${vaultCard} flex flex-wrap items-center justify-between gap-2`}><div><h1 className="text-lg font-semibold">WIKI / 개인 지식창고</h1><p className="mt-1 text-sm text-text-secondary">{repo ?? "로컬 Desktop"}</p></div><div className="flex gap-2">{tab !== "memory" && <><button className={tab === "note" ? vaultButton : vaultPrimaryButton} disabled={!vaultId || scanning || blocked || sessionOpening} type="button" onClick={() => void openSession()}>정리 세션 열기</button>{vaultId && <VaultAddSource disabled={blocked} vaultId={vaultId} host={host} onNote={async () => {}} onSaved={importSaved} />}</>}<button className={vaultButton} disabled={blocked} type="button" onClick={() => setManagement(value => !value)}>관리</button></div></header>{management && !blocked && <><div className="flex flex-wrap gap-2"><button className={vaultButton} disabled={scanning || !vaultId || tab === "memory"} type="button" onClick={() => void scan()}>{scanning ? "디렉터리 확인 중…" : "새로 고침"}</button></div><VaultSettings repo={repo} host={host} onChanged={async () => { if (isBlocked()) { blockError(); return; } await load(); }} /><VaultFolderSettings host={host} /></>}{error && <p className={vaultError} role="alert">{error}</p>}{scanMessage && <p className="text-sm text-text-secondary" role="status">{scanMessage}</p>}
    {tab === "source" && <input aria-label="자료 검색" className={vaultInput} placeholder="제목이나 내용 검색" value={query} onChange={event => { setQuery(event.target.value); setPage(0); setError(""); }} />}<nav aria-label="문서 필터" className="flex flex-wrap gap-1">{FILTERS.map(filter => <button aria-pressed={tab === filter.tab} className={vaultTab} key={filter.tab} type="button" onClick={() => changeTab(filter.tab)}>{filter.label}</button>)}</nav>
    {/* 메모리는 창고 연결과 무관하다 — 자료함이 없어도 파일은 있다(설계 R7). 그래서 !vaultId 분기보다 앞에 둔다. */}
    {tab === "memory" ? <VaultMemoryPanel agent={agent} host={host} /> : loading ? <p aria-busy="true" className="text-sm text-text-secondary">불러오는 중…</p> : !vaultId ? <><p className={vaultCard}>연결된 자료함이 없습니다.</p><VaultSettings repo={repo} host={host} onChanged={load} /></> : tab === "note" && !detail ? <WikiWorkspace key={`${host}:${vaultId}`} vaultId={vaultId} vaultRoot={vaultRoot} taskId={taskId} host={host} refreshKey={fileRefresh} onDirtyChange={updateDocumentDirty} onBusyChange={updateDocumentBusy} onMutation={scan} /> : <div className="vault-layout grid min-w-0 gap-4" data-selected={!!detail}><section className={`${vaultCard} vault-list min-w-0 `}><h2 className="font-medium">{tab === "note" ? "위키 문서" : "자료함"}</h2><p className="mt-1 text-sm text-text-secondary">{query ? `${shown.length}개 표시${hasMore ? " · 다음 페이지 있음" : ""}` : `${Math.min(shown.length - page * 100, 100)}개 표시 · 총 ${shown.length}개`}</p><div className="mt-3 divide-y divide-border">{shown.length ? (query ? shown : shown.slice(page * 100, (page + 1) * 100)).map(doc => <DocumentRow doc={doc} key={doc.id} onOpen={openDocument} />) : <EmptyList query={query} tab={tab} hasSources={docs.some(doc => doc.kind !== "note")} onCreate={() => changeTab("source")} />}</div><div className="mt-3 flex gap-2"><button className={vaultButton} disabled={page === 0} type="button" onClick={() => setPage(value => value - 1)}>이전</button><button className={vaultButton} disabled={query ? !hasMore : (page + 1) * 100 >= shown.length} type="button" onClick={() => setPage(value => value + 1)}>다음</button></div></section><div className="min-w-0">{detail && <><button className={`${vaultButton} vault-back mb-2`} type="button" onClick={closeDetail}>목록으로</button><Detail key={`${detail.document.id}:${detail.current_revision?.id ?? ""}`} detail={detail} documents={docs} host={host} repo={repo} onNavigate={id => void openDocument(id)} onDirtyChange={updateDocumentDirty} onBusyChange={updateDocumentBusy} onChanged={async () => { updateDocumentDirty(false); await load(); await openDocument(detail.document.id, true); }} onArchive={() => void archive()} /></>}</div>{detail?.document.kind === "note" && <SourceRail detail={detail} onOpen={id => void openDocument(id)} />}</div>}</section>;
}

function DocumentRow({ doc, onOpen }: { doc: VaultDocument; onOpen: (id: string) => Promise<void>; }) {
  return <div className="flex min-w-0 items-center gap-2 px-2 py-3"><button className="min-w-0 flex-1 text-left hover:bg-raised" type="button" onClick={() => void onOpen(doc.id)}><span className="block truncate text-sm">{doc.title}</span><span className="mt-1 block text-xs text-text-secondary">{scopeLabel(doc.current_scope)}</span></button></div>;
}

function EmptyList({ query, tab, hasSources, onCreate }: { query: string; tab: Tab; hasSources: boolean; onCreate: () => void; }) {
  if (query) return <p className="py-3 text-sm text-text-secondary">일치하는 자료가 없습니다.</p>;
  if (tab === "note" && hasSources) return <div className="space-y-2 py-3"><p className="text-sm text-text-secondary">위키 문서가 없습니다.</p><button className={vaultPrimaryButton} type="button" onClick={onCreate}>자료 보기</button></div>;
  return <p className="py-3 text-sm text-text-secondary">자료가 없습니다.</p>;
}

function SourceRail({ detail, onOpen }: { detail: VaultDocumentDetail; onOpen: (id: string) => void; }) {
  return <details className={`${vaultCard} vault-inspector min-w-0 text-sm`}><summary className="cursor-pointer font-medium">분석에 사용한 자료 ({detail.source_revisions.length})</summary><div className="mt-3 space-y-2">{detail.source_revisions.length ? detail.source_revisions.map(source => <button className="block w-full truncate text-left text-text-secondary hover:text-text" key={source.document_id} type="button" onClick={() => onOpen(source.document_id)}>{source.title}</button>) : <p className="text-text-secondary">연결된 자료가 없습니다.</p>}</div></details>;
}

function scopeLabel(scope: string) { return scope === "private-data" ? "나만 보기" : scope === "common" ? "모든 프로젝트" : "이 프로젝트"; }
