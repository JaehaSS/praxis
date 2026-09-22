import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MarkdownDoc } from "./ide/MarkdownDoc";
import { DirectoryPickerModal } from "./ide/DirectoryPickerModal";
import { inputCls } from "./ide/formStyles";
import { wikiConnect, wikiDocuments, wikiReadDocument, wikiSpaces, wikiSync, type WikiDocument, type WikiDocumentSummary, type WikiSpace, type WikiSyncResult } from "../lib/wiki-ipc";
interface Props { canAttach: boolean; dark: boolean; onAttach: (document: WikiDocument) => Promise<void>; }
const rootFolder = "__root__", messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));
const documentPageSize = 200;
function foldersFor(documents: WikiDocumentSummary[], spaceId: string): string[] {
  const folders = new Set<string>([rootFolder]);
  for (const document of documents) {
    if (document.space_id !== spaceId) continue;
    const parts = document.relative_path.split("/"); parts.pop();
    for (let i = 1; i <= parts.length; i += 1) folders.add(parts.slice(0, i).join("/"));
  }
  return [...folders].sort();
}

export function WikiDocumentsPanel({ canAttach, dark, onAttach }: Props) {
  const [spaces, setSpaces] = useState<WikiSpace[]>([]), [metadata, setMetadata] = useState<WikiDocumentSummary[]>([]);
  const [selectedSpaceId, setSelectedSpaceId] = useState<string | null>(null);
  const [selectedFolder, setSelectedFolder] = useState(rootFolder);
  const [query, setQuery] = useState(""), [allWiki, setAllWiki] = useState(false), [searchResults, setSearchResults] = useState<WikiDocumentSummary[] | null>(null), [truncated, setTruncated] = useState(false), [document, setDocument] = useState<WikiDocument | null>(null);
  const [loading, setLoading] = useState(true), [searching, setSearching] = useState(false), [reading, setReading] = useState(false), [syncing, setSyncing] = useState(false), [attaching, setAttaching] = useState(false), [picking, setPicking] = useState(false), [error, setError] = useState<string | null>(null), [syncResult, setSyncResult] = useState<WikiSyncResult | null>(null), [searchRevision, setSearchRevision] = useState(0), [visibleDocumentCount, setVisibleDocumentCount] = useState(documentPageSize);
  const searchVersion = useRef(0), readVersion = useRef(0), metadataVersion = useRef(0), retryRef = useRef<(() => void) | null>(null);
  const loadMetadata = useCallback(async () => {
    const version = ++metadataVersion.current;
    let nextSpaces: WikiSpace[], result: { documents: WikiDocumentSummary[]; truncated: boolean };
    try {
      [nextSpaces, result] = await Promise.all([wikiSpaces(), wikiDocuments(undefined, "")]);
    } catch (error) {
      if (version !== metadataVersion.current) return false;
      throw error;
    }
    if (version !== metadataVersion.current) return false;
    setSpaces(nextSpaces);
    setMetadata(result.documents);
    setTruncated(result.truncated);
    setSelectedSpaceId((current) => current ?? nextSpaces[0]?.id ?? null);
    return true;
  }, []);
  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    retryRef.current = null;
    try {
      if (!await loadMetadata()) return;
    } catch (nextError) {
      retryRef.current = () => void refresh();
      setError(messageOf(nextError));
    } finally {
      setLoading(false);
    }
  }, [loadMetadata]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    const version = ++searchVersion.current;
    setVisibleDocumentCount(documentPageSize);
    if (!query.trim()) {
      setSearchResults(null);
      setSearching(false);
      return;
    }
    setSearching(true);
    setError(null);
    retryRef.current = null;
    setSearchResults([]);
    setTruncated(false);
    wikiDocuments(allWiki ? undefined : selectedSpaceId ?? undefined, query)
      .then((result) => {
        if (version !== searchVersion.current) return;
        setSearchResults(result.documents);
        setTruncated(result.truncated);
      })
      .catch((nextError) => {
        if (version !== searchVersion.current) return;
        setSearchResults([]);
        setTruncated(false);
        retryRef.current = () => setSearchRevision((current) => current + 1);
        setError(messageOf(nextError));
      })
      .finally(() => {
        if (version === searchVersion.current) setSearching(false);
      });
  }, [allWiki, query, searchRevision, selectedSpaceId]);
  const folders = useMemo(() => (selectedSpaceId ? foldersFor(metadata, selectedSpaceId) : []), [metadata, selectedSpaceId]);
  const documents = useMemo(() => {
    const source = searchResults ?? metadata;
    return source.filter((item) => {
      if (allWiki) return true;
      if (item.space_id !== selectedSpaceId) return false;
      if (selectedFolder === rootFolder) return true;
      return item.relative_path.startsWith(`${selectedFolder}/`);
    });
  }, [allWiki, metadata, searchResults, selectedFolder, selectedSpaceId]);
  const visibleDocuments = useMemo(() => documents.slice(0, visibleDocumentCount), [documents, visibleDocumentCount]);
  const chooseSpace = (spaceId: string) => {
    setError(null);
    retryRef.current = null;
    readVersion.current += 1;
    setReading(false);
    setSelectedSpaceId(spaceId);
    setSelectedFolder(rootFolder);
    setAllWiki(false);
    setDocument(null);
    setVisibleDocumentCount(documentPageSize);
  };
  const chooseFolder = (folder: string) => {
    setError(null);
    retryRef.current = null;
    readVersion.current += 1;
    setReading(false);
    setSelectedFolder(folder);
    setAllWiki(false);
    setDocument(null);
    setVisibleDocumentCount(documentPageSize);
  };
  const toggleAllWiki = () => {
    setError(null);
    retryRef.current = null;
    if (!allWiki) setSelectedFolder(rootFolder);
    setAllWiki(!allWiki);
    setVisibleDocumentCount(documentPageSize);
  };
  const openDocument = async (summary: WikiDocumentSummary) => {
    const version = ++readVersion.current;
    setError(null);
    retryRef.current = null;
    setDocument(null);
    setReading(true);
    try {
      const nextDocument = await wikiReadDocument(summary.node_id);
      if (version === readVersion.current) {
        setDocument(nextDocument);
      }
    } catch (nextError) {
      if (version === readVersion.current) {
        retryRef.current = () => void openDocument(summary);
        setError(messageOf(nextError));
      }
    } finally {
      if (version === readVersion.current) setReading(false);
    }
  };
  const addFolder = async (root: string) => {
    setPicking(false);
    setSyncing(true);
    setError(null);
    retryRef.current = null;
    metadataVersion.current += 1;
    try {
      const space = await wikiConnect(root);
      setSpaces((current) => [...current.filter((item) => item.id !== space.id), space]);
      setSelectedSpaceId(space.id);
      setSelectedFolder(rootFolder);
      setAllWiki(false);
      setQuery("");
      setSearchResults(null);
      setDocument(null);
      readVersion.current += 1;
      setReading(false);
      setVisibleDocumentCount(documentPageSize);
      try {
        const result = await wikiSync();
        setSyncResult(result);
      } catch (nextError) {
        retryRef.current = () => void runSync();
        setError(`폴더는 연결됐지만 색인하지 못했습니다: ${messageOf(nextError)}`);
        return;
      }
      try {
        await loadMetadata();
      } catch (nextError) {
        retryRef.current = () => void refresh();
        setError(`폴더는 연결됐지만 색인하지 못했습니다: ${messageOf(nextError)}`);
      }
    } catch (nextError) {
      retryRef.current = () => void addFolder(root);
      setError(messageOf(nextError));
    } finally {
      setSyncing(false);
    }
  };
  const runSync = async () => {
    setSyncing(true);
    setError(null);
    retryRef.current = null;
    metadataVersion.current += 1;
    try {
      const result = await wikiSync();
      setSyncResult(result);
    } catch (nextError) {
      retryRef.current = () => void runSync();
      setError(messageOf(nextError));
      setSyncing(false);
      return;
    }
    try {
      readVersion.current += 1;
      setReading(false);
      setDocument(null);
      setVisibleDocumentCount(documentPageSize);
      setSearchRevision((current) => current + 1);
      try {
        await loadMetadata();
      } catch (nextError) {
        retryRef.current = () => void refresh();
        setError(messageOf(nextError));
      }
    } catch (nextError) {
      setError(messageOf(nextError));
    } finally {
      setSyncing(false);
    }
  };
  const attach = async () => {
    if (!document || !canAttach || attaching) return;
    setAttaching(true);
    setError(null);
    retryRef.current = null;
    try {
      await onAttach(document);
    } catch (nextError) {
      retryRef.current = () => void attach();
      setError(messageOf(nextError));
    } finally {
      setAttaching(false);
    }
  };
  return (
    <div className="flex-1 min-h-0 flex flex-col p-4 gap-3 overflow-hidden">
      <header className="shrink-0 flex flex-wrap items-center gap-2">
        <div className="min-w-0 mr-auto">
          <h1 className="text-lg font-semibold">Wiki</h1>
          <p className="text-sm text-text-muted">로컬 Markdown을 찾고 현재 작업에 문서 참조를 붙입니다.</p>
        </div>
        <button className="h-8 px-3 rounded-md border border-border text-sm text-text-secondary hover:text-text" onClick={() => setPicking(true)}>
          폴더 연결
        </button>
        <button className="h-8 px-3 rounded-md bg-primary text-sm text-white disabled:opacity-40" disabled={syncing || spaces.length === 0} onClick={() => void runSync()}>
          {syncing ? "색인 중…" : "색인 갱신"}
        </button>
      </header>
      <div className="shrink-0 flex flex-wrap gap-2 items-center">
        <input className={`${inputCls} flex-1 min-w-52`} value={query} onChange={(event) => setQuery(event.target.value)} placeholder="제목과 본문 검색" aria-label="Wiki 검색" />
        <button className={`h-8 px-2.5 rounded-md text-sm ${allWiki ? "bg-raised text-text" : "border border-border text-text-secondary"}`} onClick={toggleAllWiki}>
          {allWiki ? "전체 Wiki" : "선택 그룹"}
        </button>
        {searching && <span className="text-xs text-text-muted">검색 중…</span>}
      </div>
      {error && <div className="shrink-0 text-sm text-status-failed">{error}{retryRef.current && <button className="ml-1 underline" onClick={() => retryRef.current?.()}>다시 시도</button>}</div>}
      {syncResult && <div className="shrink-0 text-xs text-text-muted">색인 {syncResult.indexed} · 변경 없음 {syncResult.skipped} · 삭제 {syncResult.deleted} · 링크 {syncResult.edges}{syncResult.complete ? "" : " · 일부만 완료"}{syncResult.warnings.length ? ` · ${syncResult.warnings.join(" · ")}` : ""}</div>}
      {truncated && <div className="shrink-0 text-xs text-text-muted">표시 가능한 문서 수를 넘겨 일부 결과만 보입니다.</div>}
      <div className="flex-1 min-h-0 grid grid-cols-[minmax(10rem,16rem)_minmax(12rem,20rem)_minmax(0,1fr)] gap-3 max-[780px]:grid-cols-1 max-[780px]:overflow-auto">
        <nav className="border border-border rounded-md overflow-auto p-2" aria-label="Wiki 그룹">
          {loading ? <p className="text-sm text-text-muted">Wiki를 불러오는 중…</p> : spaces.length === 0 ? <p className="text-sm text-text-muted">연결된 폴더가 없습니다.</p> : spaces.map((space) => (
            <div key={space.id} className="mb-2">
              <button className={`w-full text-left px-2 py-1.5 rounded text-sm ${space.id === selectedSpaceId ? "bg-raised text-text" : "text-text-secondary hover:text-text"}`} onClick={() => chooseSpace(space.id)} title={space.root}>{space.name}</button>
              {space.id === selectedSpaceId && folders.map((folder) => <button key={folder} className={`block w-full text-left px-3 py-1 text-xs truncate ${folder === selectedFolder ? "text-primary-bright" : "text-text-muted hover:text-text"}`} onClick={() => chooseFolder(folder)}>{folder === rootFolder ? "루트" : folder}</button>)}
            </div>
          ))}
        </nav>
        <section className="border border-border rounded-md overflow-auto" aria-label="Wiki 문서">
          {documents.length === 0 ? <p className="p-3 text-sm text-text-muted">{query ? "일치하는 문서가 없습니다." : "이 폴더에는 문서가 없습니다."}</p> : <>{visibleDocuments.map((item) => <button key={item.node_id} className={`block w-full text-left p-3 border-b border-border last:border-b-0 hover:bg-raised ${document?.node_id === item.node_id ? "bg-raised" : ""}`} onClick={() => void openDocument(item)}><span className="block text-sm text-text truncate">{item.title}</span><span className="block mt-1 text-xs text-text-muted truncate">{item.relative_path}</span>{item.snippet && <span className="block mt-1 text-xs text-text-secondary line-clamp-2">{item.snippet}</span>}</button>)}{visibleDocuments.length < documents.length && <button className="w-full p-3 text-sm text-primary-bright hover:bg-raised" onClick={() => setVisibleDocumentCount((current) => current + documentPageSize)}>더 보기</button>}</>}
        </section>
        <section className="border border-border rounded-md min-w-0 flex flex-col overflow-hidden" aria-label="문서 미리보기">
          {document ? <><div className="shrink-0 flex items-center gap-2 px-3 py-2 border-b border-border"><span className="min-w-0 flex-1 text-sm font-medium truncate" title={document.path}>{document.title}</span><button className="h-7 px-2 rounded bg-primary text-xs text-white disabled:opacity-40" disabled={!canAttach || attaching} title={canAttach ? "현재 로컬 작업에 참조를 덧붙입니다" : "로컬 작업을 선택한 뒤 참조를 붙일 수 있습니다"} onClick={() => void attach()}>{attaching ? "확인 중…" : "작업에 참조"}</button></div><div className="flex-1 overflow-auto p-3"><MarkdownDoc text={document.body} dark={dark} loadImages={false} /></div></> : <p className="p-3 text-sm text-text-muted">{reading ? "문서를 불러오는 중…" : "문서를 선택하면 내용을 미리 봅니다."}</p>}
        </section>
      </div>
      {picking && <DirectoryPickerModal initialPath={spaces[0]?.root} title="Wiki 폴더 연결" onPick={(root) => void addFolder(root)} onClose={() => setPicking(false)} />}
    </div>
  );
}
