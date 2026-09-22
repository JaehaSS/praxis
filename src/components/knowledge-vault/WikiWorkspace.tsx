import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { HostId } from "../../lib/transport";
import { wikiGraph, wikiRead, wikiSave, wikiTrash, type WikiGraph } from "../../lib/wiki-workspace-ipc";
import { resolveDocumentLink } from "../../lib/document-link";
import { vaultSettingsGet } from "../../lib/knowledge-vault-ipc";
import { DEFAULT_WIKI_HOME, entryPage } from "../../lib/wiki-entry";
import { folderColor, folderGroups, folderSlotOf } from "../../lib/wiki-folder-groups";
import { buildWikiSearchIndex, searchWikiPages, type WikiMatch } from "../../lib/wiki-search";
import { buildWikiCapture } from "../../lib/designmode/selection-capture";
import { pushCapture } from "../../lib/designmode/store";
import { MarkdownDoc } from "../ide/MarkdownDoc";
import { WikiGraphCanvas } from "./WikiGraphCanvas";
import { vaultButton, vaultCard, vaultError, vaultInput, vaultPrimaryButton, vaultTextarea } from "./ui";
import "./WikiWorkspace.css";

type Draft = { path: string; content: string; base: string; sha256: string | null };
interface Props {
  vaultId: string; host: HostId; refreshKey?: number;
  /** 첨부가 만들 절대 경로의 앞부분. 비어 있으면 첨부할 수 없다. */
  vaultRoot?: string;
  /** 첨부를 받을 세션. 위키는 세션 바깥에서도 열리므로 없을 수 있다. */
  taskId?: number | null;
  onDirtyChange?: (dirty: boolean) => void; onBusyChange?: (busy: boolean) => void;
  onMutation?: () => Promise<void>;
}
/** 원문 보기가 그리는 최대 줄 수. 문서 상한이 2MiB라, 한 문서가 수만 줄일 때 DOM을 그만큼 만들지 않는다. */
const MAX_SOURCE_LINES = 2000;
/**
 * 검색이 어디서 맞았는지 보여주는 한 줄.
 *
 * 제목과 경로는 바로 위에 이미 그려지므로 되풀이하지 않는다. 눈에 안 보이던 별칭과 본문만 남긴다.
 */
function MatchLine({ match }: { match: WikiMatch | null }) {
  if (match?.field !== "alias" && match?.field !== "body") return null;
  return <span className="mt-0.5 block truncate text-xs text-text-secondary">
    {match.field === "alias" && <span className="text-text-muted">별칭 · </span>}
    {match.before}<mark className="rounded-sm bg-primary/20 px-0.5 text-text">{match.text}</mark>{match.after}
  </span>;
}

/** 진입 문서 설정을 읽는다. 읽기 실패로 위키를 못 여는 일은 없어야 하므로 기본값으로 간다. */
async function homeSetting(host: HostId) {
  try { return (await vaultSettingsGet(host)).wiki_home || DEFAULT_WIKI_HOME; }
  catch { return DEFAULT_WIKI_HOME; }
}

export function WikiWorkspace({ vaultId, host, vaultRoot = "", taskId = null, refreshKey = 0, onDirtyChange, onBusyChange, onMutation }: Props) {
  const [graph, setGraph] = useState<WikiGraph | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [query, setQuery] = useState(""); const [tag, setTag] = useState(""); const [kind, setKind] = useState("");
  const [sourceOpen, setSourceOpen] = useState(false); const [activeLine, setActiveLine] = useState<number | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [discard, setDiscard] = useState(false); const [deleting, setDeleting] = useState(false);
  const [showGraph, setShowGraph] = useState(true); const [depth, setDepth] = useState("all");
  const [busy, setBusy] = useState(false); const [error, setError] = useState(""); const [notice, setNotice] = useState("");
  const [stale, setStale] = useState(false);
  const request = useRef(0); const alive = useRef(true); const working = useRef(false); const draftRef = useRef(draft);
  draftRef.current = draft;
  const scopeKey = `${host}:${vaultId}`;
  const scope = useRef(scopeKey); scope.current = scopeKey;
  const dirty = draft !== null;
  const setWorking = useCallback((value: boolean) => { working.current = value; setBusy(value); onBusyChange?.(value); }, [onBusyChange]);
  const setEditing = (value: Draft | null) => { draftRef.current = value; setDraft(value); onDirtyChange?.(value !== null); };
  useEffect(() => { onDirtyChange?.(dirty); return () => onDirtyChange?.(false); }, [dirty, onDirtyChange]);
  useEffect(() => { alive.current = true; return () => { alive.current = false; request.current++; onBusyChange?.(false); }; }, [onBusyChange]);

  const refresh = useCallback(async (message = "") => {
    const token = ++request.current;
    setWorking(true); setError("");
    try {
      const [next, home] = await Promise.all([wikiGraph(vaultId, host), homeSetting(host)]);
      if (!alive.current || request.current !== token) return false;
      setGraph(next); setStale(false);
      // 보고 있던 문서가 그대로 있으면 지킨다. 없으면(첫 진입·삭제 직후) 진입 문서를 연다 —
      // 창고를 고른 것으로 끝나고, 나머지는 링크를 타고 들어간다.
      setSelected(id => (id && next.nodes.some(page => page.id === id) ? id : entryPage(next.nodes, home)));
      if (message) setNotice(message);
      return true;
    } catch (reason) {
      if (alive.current && request.current === token) { setStale(true); setError(`${message ? `${message} ` : ""}문서 목록 갱신에 실패했습니다` + `: ${String(reason)}`); }
      return false;
    } finally { if (alive.current && request.current === token) setWorking(false); }
  }, [vaultId, host, setWorking]);
  useEffect(() => {
    request.current++; setGraph(null); setSelected(null); setDraft(null); draftRef.current = null; setError(""); setNotice("");
    setDeleting(false); setDiscard(false); setQuery(""); setTag(""); setKind("");
    void refresh();
    return () => { request.current++; };
  }, [refresh]);
  const previousRefresh = useRef(refreshKey);
  useEffect(() => {
    if (previousRefresh.current === refreshKey) return;
    previousRefresh.current = refreshKey;
    if (!draftRef.current && !working.current) void refresh();
  }, [refreshKey, refresh]);

  const pages = graph?.nodes ?? [];
  const current = pages.find(page => page.id === selected);
  // 본문 접기는 문서 집합이 바뀔 때만 한다 — 타이핑마다 2MiB를 다시 접지 않으려는 것이다.
  const searchIndex = useMemo(() => buildWikiSearchIndex(pages), [pages]);
  const results = useMemo(() => searchWikiPages(searchIndex, pages.filter(page => (!tag || page.tags.includes(tag)) && (!kind || page.type === kind)), query), [searchIndex, pages, query, tag, kind]);
  const filtered = useMemo(() => results.map(result => result.page), [results]);
  // 폴더 색은 걸러진 목록이 아니라 창고 전체로 정한다. 거르는 순간 색이 바뀌면 색이 소속이 아니라 필터를 뜻하게 된다.
  const groups = useMemo(() => folderGroups(pages.map(page => page.path)), [pages]);
  const graphPages = useMemo(() => {
    if (depth === "all" || !selected) return filtered;
    const ids = new Set([selected]); let frontier = [selected];
    for (let i = 0; i < Number(depth); i++) { const next: string[] = []; for (const id of frontier) { const page = pages.find(p => p.id === id); for (const link of [...(page?.outgoing ?? []), ...(page?.backlinks ?? [])]) { if (!ids.has(link)) { ids.add(link); next.push(link); } } } frontier = next; }
    return filtered.filter(page => ids.has(page.id));
  }, [filtered, pages, depth, selected]);
  const tags = Array.from(new Set(pages.flatMap(page => page.tags))).sort();
  const kinds = Array.from(new Set(pages.map(page => page.type).filter(Boolean))).sort();
  const titleOf = (id: string) => pages.find(page => page.id === id)?.title ?? id;
  // 이 문서가 건 링크의 근거. 줄 번호는 원문(source_prefix + body) 기준이라 원문 보기와 바로 맞는다.
  const cites = useMemo(() => (graph?.edges ?? [])
    .filter(edge => edge.source === selected)
    .flatMap(edge => edge.evidence.map(item => ({ ...item, page: edge.target })))
    .sort((a, b) => a.line - b.line), [graph, selected]);
  // 이 문서를 가리킨 근거. 줄 번호는 상대 문서의 원문 기준이므로, 눌렀을 때 그 문서를 열어야 한다.
  const citedBy = useMemo(() => (graph?.edges ?? [])
    .filter(edge => edge.target === selected)
    .flatMap(edge => edge.evidence.map(item => ({ ...item, page: edge.source })))
    .sort((a, b) => a.page.localeCompare(b.page) || a.line - b.line), [graph, selected]);
  const sourceLines = useMemo(() => current ? `${current.source_prefix}${current.body}`.split("\n") : [], [current]);
  const lineNode = useRef<HTMLLIElement | null>(null);
  // 다른 문서의 근거를 눌러 이동할 때, 문서가 바뀐 뒤에야 그 줄로 갈 수 있다.
  const pendingLine = useRef<number | null>(null);
  const select = (id: string) => {
    if (working.current || draftRef.current) { setNotice("편집을 저장하거나 취소한 뒤 문서를 이동하세요."); return false; }
    setSelected(id); setDeleting(false); setNotice("");
    return true;
  };
  useEffect(() => {
    const line = pendingLine.current; pendingLine.current = null;
    setSourceOpen(line !== null); setActiveLine(line);
  }, [selected]);
  useEffect(() => {
    if (sourceOpen && activeLine !== null) lineNode.current?.scrollIntoView?.({ block: "center" });
  }, [sourceOpen, activeLine]);
  /** 근거 한 줄로 이동한다. 같은 문서면 원문 보기를 열고, 다른 문서면 이동한 뒤 그 줄을 연다. */
  /**
   * 읽던 문서를 대화 입력창에 붙인다.
   *
   * 에디터의 선택 첨부(⌘L)와 같은 캡처 칩 경로를 탄다 — 전송 배관을 새로 만들지 않는다.
   * 본문은 프런트매터까지 실어 보낸다. 종류·상태·별칭이 거기 있고, 원문 보기가 보여 주는 것과
   * 같아야 사람과 에이전트가 같은 것을 본다.
   */
  const attach = () => {
    if (!current || taskId === null || !vaultRoot) return;
    pushCapture(taskId, buildWikiCapture({
      taskId,
      filePath: `${vaultRoot.replace(/\/+$/, "")}/${current.path}`,
      title: current.title,
      body: `${current.source_prefix}${current.body}`,
    }));
    setNotice(`"${current.title}" 문서를 대화에 첨부했습니다. 대화 화면의 입력창에서 확인하세요.`);
  };
  const goToLine = (pageId: string, line: number) => {
    if (pageId === current?.id) { setSourceOpen(true); setActiveLine(line); return; }
    pendingLine.current = line;
    if (!select(pageId)) pendingLine.current = null;
  };
  const edit = async () => {
    if (!current || working.current || draftRef.current || stale || !graph?.writable) return;
    const token = ++request.current;
    setWorking(true); setError(""); setDeleting(false);
    try {
      const file = await wikiRead(vaultId, current.path, host);
      if (alive.current && token === request.current) setEditing({ ...file, base: file.content });
    } catch (reason) { if (alive.current && token === request.current) setError(String(reason)); }
    finally { if (alive.current && token === request.current) setWorking(false); }
  };
  const create = () => {
    if (working.current || draftRef.current || !graph?.writable || stale) return;
    const parent = current?.path.includes("/") ? current.path.slice(0, current.path.lastIndexOf("/") + 1) : "";
    setEditing({ path: `${parent}새 문서.md`, content: "# 새 문서\n\n", base: "", sha256: null });
    setDiscard(false); setDeleting(false); setError("");
  };
  const mutationFinished = async (message: string) => {
    setNotice(message);
    await refresh(message);
    if (onMutation && alive.current && scope.current === scopeKey) {
      try { await onMutation(); }
      catch (reason) { if (alive.current) setNotice(`${message} 기존 자료 색인 갱신 실패: ${String(reason)}`); }
    }
  };
  const save = async () => {
    const value = draftRef.current;
    if (!value || working.current || !value.path.trim()) return;
    const token = ++request.current;
    setWorking(true); setError("");
    try {
      const file = await wikiSave(vaultId, value.path, value.content, value.sha256, host);
      if (!alive.current || token !== request.current) return;
      setEditing(null); setDiscard(false); setSelected(file.path.normalize("NFC"));
      await mutationFinished("문서를 저장했습니다.");
    } catch (reason) {
      if (alive.current && token === request.current) { setError(String(reason)); setWorking(false); }
    }
  };
  const remove = async () => {
    if (!current || working.current || draftRef.current || stale || !graph?.writable) return;
    const token = ++request.current;
    setWorking(true); setError("");
    try {
      await wikiTrash(vaultId, current.path, current.sha256, host);
      if (!alive.current || token !== request.current) return;
      setDeleting(false); setSelected(null);
      await mutationFinished("문서를 휴지통으로 옮겼습니다.");
    } catch (reason) { if (alive.current && token === request.current) { setError(String(reason)); setWorking(false); } }
  };
  const openLink = (href: string) => {
    if (!current) return;
    const link = resolveDocumentLink(href, current.path);
    const target = link?.kind === "file" ? pages.find(page => page.id === link.path.normalize("NFC")) : null;
    if (target) select(target.id);
    else setNotice(link?.kind === "url" ? "외부 링크입니다. 원문에서 주소를 확인할 수 있습니다." : "연결 대상 문서를 찾을 수 없습니다. 연결 점검을 확인하세요.");
  };
  const relation = (ids: string[], label: string) => <section><h3 className="mt-4 mb-2 text-sm font-medium">{label} ({ids.length})</h3><div className="flex flex-wrap gap-2">{ids.map(id => <button className={vaultButton} type="button" key={id} disabled={busy || dirty} onClick={() => select(id)}>{pages.find(p => p.id === id)?.title ?? id}</button>)}{!ids.length && <p className="text-xs text-text-muted">연결된 문서가 없습니다.</p>}</div></section>;

  return <section aria-label="문서와 그래프" className="space-y-3" aria-busy={busy}>
    <div className="flex flex-wrap items-center gap-2"><h2 className="mr-auto font-semibold">위키 문서</h2><button className={vaultPrimaryButton} type="button" disabled={busy || dirty || !graph?.writable || stale} onClick={create}>새 문서</button><button className={vaultButton} type="button" disabled={busy || dirty} onClick={() => void refresh()}>문서 새로 고침</button><button className={vaultButton} type="button" aria-pressed={showGraph} onClick={() => setShowGraph(v => !v)}>그래프 {showGraph ? "접기" : "보기"}</button></div>
    {busy && <p role="status" className="text-sm text-text-secondary">문서를 확인하고 있습니다…</p>}
    {error && <p className={vaultError} role="alert">{error}</p>}
    {notice && <p className="text-sm text-text-secondary" role="status">{notice}</p>}
    {stale && graph && <p className="text-sm text-status-awaiting">이전 목록입니다. 새로 고침이 성공하면 편집할 수 있습니다.</p>}
    {graph && !graph.writable && <p className="text-sm text-text-secondary">읽기 전용 창고입니다.</p>}
    <div className="wiki-workspace-grid">
      <aside className={`${vaultCard} space-y-3`} aria-label="위키 문서 목록">
        <input className={vaultInput} aria-label="위키 문서 검색" placeholder="제목, 본문, 별칭 검색" value={query} onChange={e => setQuery(e.target.value)} />
        <select className={vaultInput} aria-label="위키 태그" value={tag} onChange={e => setTag(e.target.value)}><option value="">모든 태그</option>{tags.map(t => <option key={t}>{t}</option>)}</select>
        {kinds.length > 1 && <select className={vaultInput} aria-label="위키 문서 종류" value={kind} onChange={e => setKind(e.target.value)}><option value="">모든 종류</option>{kinds.map(k => <option key={k}>{k}</option>)}</select>}
        <p className="text-xs text-text-secondary">{filtered.length}개 표시 · 전체 {pages.length}개</p>
        <nav className="max-h-[65vh] space-y-1 overflow-auto">{results.map(({ page, match }) => <button key={page.id} type="button" data-page={page.id} aria-current={selected === page.id ? "page" : undefined} className={`block w-full rounded-md p-2 text-left hover:bg-raised ${selected === page.id ? "bg-raised" : ""}`} onClick={() => select(page.id)}><span className="flex items-center gap-1.5 text-sm"><span aria-hidden className="h-2 w-2 shrink-0 rounded-full" style={{ background: folderColor(folderSlotOf(groups, page.path)) }} /><span className="min-w-0 break-words">{page.title}</span></span><span className="block break-all text-xs text-text-muted">{page.path}</span><MatchLine match={match} /></button>)}</nav>
        {!filtered.length && graph && <p className="text-sm text-text-secondary">{query || tag || kind ? "검색 결과가 없습니다." : "Markdown 문서가 없습니다. 새 문서를 만들어 보세요."}</p>}
      </aside>
      <div className="min-w-0 space-y-3">
        {showGraph && graph && <><select className={vaultInput} aria-label="그래프 연결 범위" value={depth} onChange={e => setDepth(e.target.value)}><option value="all">전체 연결</option><option value="1">선택 문서의 1단계 연결</option><option value="2">선택 문서의 2단계 연결</option></select><WikiGraphCanvas pages={graphPages} edges={graph.edges} selected={selected} groups={groups} onSelect={select} /></>}
        <article className={vaultCard} aria-label="위키 문서 읽기">
          {draft ? <div className="space-y-3"><h2 className="font-semibold">{draft.sha256 ? "문서 편집" : "새 문서 작성"}</h2><label className="block text-sm">파일 경로<input className={vaultInput} aria-label="문서 파일 경로" disabled={busy || draft.sha256 !== null} value={draft.path} onChange={e => setEditing({ ...draft, path: e.target.value })} /></label>{!draft.sha256 && <p className="text-xs text-text-secondary">기존 폴더 아래의 .md 경로를 입력하세요.</p>}<textarea aria-label="문서 원문" className={`${vaultTextarea} min-h-[24rem] font-code`} disabled={busy} value={draft.content} onChange={e => setEditing({ ...draft, content: e.target.value })} /><div className="flex gap-2"><button className={vaultPrimaryButton} type="button" disabled={busy || !draft.path.trim()} onClick={() => void save()}>문서 저장</button><button className={vaultButton} type="button" disabled={busy} onClick={() => { if (draft.content !== draft.base || !draft.sha256) setDiscard(true); else setEditing(null); }}>편집 취소</button></div>{discard && <div role="alert" className="space-y-2"><p>저장하지 않은 수정을 버릴까요?</p><button className={vaultButton} type="button" disabled={busy} onClick={() => setDiscard(false)}>계속 편집</button><button className={vaultButton} type="button" disabled={busy} onClick={() => { setEditing(null); setDiscard(false); }}>수정 버리기</button></div>}</div> : current ? <>
            <div className="flex flex-wrap items-center gap-2"><h2 className="mr-auto break-words text-xl font-semibold">{current.title}</h2><button className={vaultButton} type="button" disabled={busy || stale || !graph?.writable} onClick={() => void edit()}>원문 편집</button><button className={vaultButton} type="button" disabled={busy || stale || !graph?.writable} onClick={() => setDeleting(true)}>삭제</button><button className={vaultButton} type="button" disabled={taskId === null || !vaultRoot} title={taskId === null ? "대화를 하나 연 뒤에 첨부할 수 있습니다." : !vaultRoot ? "창고 경로를 읽지 못해 첨부할 수 없습니다." : undefined} onClick={attach}>대화에 첨부</button></div><p className="my-2 break-all text-xs text-text-muted">{current.path}</p>
            <div className="my-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-secondary">
              {[["종류", current.type], ["상태", current.status], ["범위", current.scope]].filter(([, value]) => value).map(([name, value]) => <span key={name}>{name} <span className="text-text">{value}</span></span>)}
              {current.tags.map(name => <button key={name} type="button" aria-label={`태그로 거르기: ${name}`} aria-pressed={tag === name} className={`rounded-full border border-border px-2 py-0.5 ${tag === name ? "bg-raised text-text" : ""}`} onClick={() => setTag(tag === name ? "" : name)}>#{name}</button>)}
            </div>
            {deleting && <div role="alert" className="my-3 space-y-2 rounded-md border border-border p-3"><p className="text-sm">{current.path}을 휴지통으로 옮길까요? 이 문서를 참조하는 문서 {current.backlinks.length}개의 링크가 끊어집니다.</p><button className={vaultButton} type="button" disabled={busy} onClick={() => setDeleting(false)}>삭제 취소</button><button className={vaultButton} type="button" disabled={busy} onClick={() => void remove()}>휴지통으로 이동</button></div>}
            <MarkdownDoc text={current.body} dark loadImages={false} onOpenLink={openLink} />
            {relation(current.outgoing, "관련 문서")}{relation(current.backlinks, "이 문서를 참조하는 문서")}
            {!!(cites.length + citedBy.length) && <details className="mt-4 rounded-md border border-border p-3">
              <summary className="cursor-pointer text-sm">연결 근거 ({cites.length + citedBy.length})</summary>
              {!!cites.length && <section><h3 className="mb-2 mt-3 text-sm font-medium">이 문서가 건 링크 ({cites.length})</h3>
                <ul className="space-y-1 text-xs">{cites.map((item, i) => <li key={`cite-${i}`} className="flex flex-wrap items-center gap-2">
                  <button type="button" className={vaultButton} disabled={busy || dirty} onClick={() => goToLine(current.id, item.line)}>{item.line}번째 줄</button>
                  <span className="break-words">→ {titleOf(item.page)}</span>
                  <code className="break-all text-text-muted">{item.target}{item.anchor && `#${item.anchor}`} · {item.syntax}</code>
                </li>)}</ul></section>}
              {!!citedBy.length && <section><h3 className="mb-2 mt-3 text-sm font-medium">이 문서를 가리킨 자리 ({citedBy.length})</h3>
                <ul className="space-y-1 text-xs">{citedBy.map((item, i) => <li key={`cited-${i}`} className="flex flex-wrap items-center gap-2">
                  <button type="button" className={vaultButton} disabled={busy || dirty} onClick={() => goToLine(item.page, item.line)}>{titleOf(item.page)} {item.line}번째 줄</button>
                  <code className="break-all text-text-muted">{item.target}{item.anchor && `#${item.anchor}`} · {item.syntax}</code>
                </li>)}</ul></section>}
            </details>}
            <details className="mt-4 rounded-md border border-border p-3" open={sourceOpen} onToggle={event => setSourceOpen(event.currentTarget.open)}>
              <summary className="cursor-pointer text-sm">원문 보기 ({sourceLines.length}줄)</summary>
              {activeLine !== null && activeLine > MAX_SOURCE_LINES && <p role="status" className="mt-2 text-xs text-status-awaiting">{activeLine}번째 줄은 표시 범위를 넘습니다. 원문 편집에서 확인하세요.</p>}
              <ol className="mt-3 max-h-[28rem] overflow-auto font-code text-xs leading-5">{sourceLines.slice(0, MAX_SOURCE_LINES).map((text, index) => {
                const line = index + 1, active = line === activeLine;
                return <li key={line} ref={active ? lineNode : undefined} aria-current={active ? "true" : undefined} className={`flex gap-3 ${active ? "rounded bg-raised" : ""}`}>
                  <span className="w-10 shrink-0 select-none text-right text-text-muted">{line}</span>
                  <code className="min-w-0 whitespace-pre-wrap break-all">{text}</code>
                </li>;
              })}</ol>
              {sourceLines.length > MAX_SOURCE_LINES && <p className="mt-2 text-xs text-text-secondary">앞의 {MAX_SOURCE_LINES}줄을 표시합니다.</p>}
            </details>
          </> : <p className="py-8 text-sm text-text-secondary">목록이나 그래프에서 문서를 선택하세요.</p>}
        </article>
        {!!graph?.diagnostics.length && <details className={vaultCard}><summary className="cursor-pointer text-sm">연결 점검 ({graph.diagnostics.length})</summary><ul className="mt-3 space-y-2 text-xs">{graph.diagnostics.slice(0, 100).map((item, i) => <li key={i}>{item.source} → {item.target || "읽기 불가"} · {item.kind}</li>)}</ul>{graph.diagnostics.length > 100 && <p className="text-xs">앞의 100건을 표시합니다.</p>}</details>}
      </div>
    </div>
  </section>;
}
