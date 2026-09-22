import { useCallback, useId, useMemo, useRef, useState } from "react";
import type { WikiEdge, WikiPage } from "../../lib/wiki-workspace-ipc";
import { OTHER_FOLDER_LABEL, folderColor, folderLabel, folderSlotOf, type FolderGroup } from "../../lib/wiki-folder-groups";
import type { Wiki3DView } from "./wiki-graph-3d";
import { WikiGraph3D } from "./WikiGraph3D";
import { vaultButton, vaultTab } from "./ui";

/** 라이트 모드의 세 번째 색은 흰 배경 대비가 3:1에 못 미친다. 범례는 그 2차 부호라 선택이 아니라 의무다. */
function FolderLegend({ groups, other }: { groups: readonly FolderGroup[]; other: boolean }) {
  const entries = [
    ...groups.map(group => ({ key: group.folder, name: folderLabel(group.folder), slot: group.slot as number | null })),
    ...(other ? [{ key: "\u0000other", name: OTHER_FOLDER_LABEL, slot: null }] : []),
  ];
  if (entries.length < 2) return null;
  return <ul aria-label="폴더 색 범례" className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-secondary">
    {entries.map(entry => <li key={entry.key} className="flex min-w-0 items-center gap-1.5">
      <span aria-hidden className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: folderColor(entry.slot) }} />
      <span className="truncate">{entry.name}</span>
    </li>)}
  </ul>;
}

export function WikiGraphCanvas({ pages, edges, selected, groups, onSelect }: { pages: WikiPage[]; edges: WikiEdge[]; selected: string | null; groups: readonly FolderGroup[]; onSelect: (id: string) => void }) {
  const [zoom, setZoom] = useState(1);
  const [mode, setMode] = useState<"2d" | "3d">("3d");
  const [failed, setFailed] = useState(false);
  const controller = useRef<Wiki3DView | null>(null);
  const fail = useCallback(() => { setFailed(true); setMode("2d"); }, []);
  const marker = `wiki-arrow-${useId().replace(/:/g, "")}`;
  const visible = useMemo(() => {
    const chosen = pages.find(page => page.id === selected);
    const first = pages.slice(0, 200);
    // Changing selection inside the visible set must not reorder the simulation.
    return chosen && !first.includes(chosen) ? [...first.slice(0, 199), chosen] : first;
  }, [pages, selected]);
  const positions = useMemo(() => new Map(visible.map((page, i, shown) => {
    const angle = 2 * Math.PI * i / Math.max(1, shown.length) - Math.PI / 2;
    const radius = shown.length === 1 ? 0 : 175 + (i % 3) * 25;
    return [page.id, { x: 340 + Math.cos(angle) * radius, y: 255 + Math.sin(angle) * radius }];
  })), [visible]);
  const hasOther = useMemo(() => visible.some(page => folderSlotOf(groups, page.path) === null), [visible, groups]);
  return <section aria-label="위키 관계 그래프" className="rounded-lg border border-border bg-bg p-2">
    <div className="flex flex-wrap items-center gap-2 text-xs text-text-secondary">
      <span className="mr-auto">{pages.length}개 문서 · 화살표는 참조 방향</span>
      <div className="flex rounded-md border border-border" role="group" aria-label="그래프 표시 방식">
        <button type="button" className={vaultTab} aria-pressed={mode === "2d"} onClick={() => setMode("2d")}>2D</button>
        <button type="button" className={vaultTab} aria-pressed={mode === "3d"} onClick={() => { setFailed(false); setMode("3d"); }}>3D</button>
      </div>
      <button type="button" className={vaultButton} aria-label="그래프 축소" onClick={() => mode === "3d" ? controller.current?.zoom(1.25) : setZoom(z => Math.max(0.5, z - 0.25))}>−</button>
      <button type="button" className={vaultButton} aria-label="그래프 확대" onClick={() => mode === "3d" ? controller.current?.zoom(0.8) : setZoom(z => Math.min(3, z + 0.25))}>+</button>
      <button type="button" className={vaultButton} onClick={() => mode === "3d" ? controller.current?.reset() : setZoom(1)}>{mode === "3d" ? "화면 맞춤" : "배율 초기화"}</button>
    </div>
    {failed && <p role="status" className="mt-2 text-xs text-text-secondary">3D 그래프를 표시할 수 없어 2D로 전환했습니다. 3D 버튼으로 다시 시도할 수 있습니다.</p>}
    {pages.length > 200 && <p role="status" className="text-xs text-text-secondary">그래프에는 선택 문서를 포함해 최대 200개를 표시합니다. 검색이나 연결 범위로 좁히세요.</p>}
    {!pages.length ? <p className="p-4 text-text-secondary">표시할 문서가 없습니다.</p> : mode === "3d" ? <>
      <WikiGraph3D pages={visible} edges={edges} selected={selected} groups={groups} onSelect={onSelect} onFailure={fail} controller={controller} />
      <FolderLegend groups={groups} other={hasOther} />
      <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-text-secondary">
        <label className="flex min-w-0 max-w-full items-center gap-2"><span className="shrink-0">문서 이동</span><select aria-label="3D 그래프 문서 선택" className="min-w-0 max-w-full rounded border border-border bg-surface p-1 text-text" value={visible.some(page => page.id === selected) ? selected! : ""} onChange={event => { if (event.target.value) onSelect(event.target.value); }}><option value="">문서 선택</option>{visible.map(page => <option key={page.id} value={page.id}>{page.title}</option>)}</select></label>
        <span>드래그로 회전 · 휠로 확대 · 우클릭 드래그로 이동</span>
      </div>
    </> : <><div className="max-h-[32rem] overflow-auto"><svg viewBox="0 0 680 520" style={{ width: `${zoom * 100}%`, height: 320 * zoom, minWidth: 450 * zoom }} aria-label="문서 간 참조 관계">
      <defs><marker id={marker} viewBox="0 0 10 10" refX="16" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" fill="currentColor" /></marker></defs>
      {edges.map(edge => { const a = positions.get(edge.source), b = positions.get(edge.target); return a && b ? <line key={`${edge.source}:${edge.target}`} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke="currentColor" opacity={selected === edge.source || selected === edge.target ? 0.8 : 0.2} className="text-primary-bright" markerEnd={`url(#${marker})`} /> : null; })}
      {visible.map(page => { const p = positions.get(page.id)!; return <g key={page.id} role="button" tabIndex={0} aria-label={`그래프 문서: ${page.title}`} aria-pressed={selected === page.id} transform={`translate(${p.x},${p.y})`} onClick={() => onSelect(page.id)} onKeyDown={e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onSelect(page.id); } }} className="cursor-pointer outline-none focus:opacity-60"><title>{page.path}</title><circle r="14" fill="transparent" /><circle r={selected === page.id ? 7 : 4} fill={selected === page.id ? "var(--c-primary-bright)" : folderColor(folderSlotOf(groups, page.path))} /><text y="20" textAnchor="middle" fill="currentColor" className="text-text text-[14px]">{page.title.length > 17 ? `${page.title.slice(0, 17)}…` : page.title}</text></g>; })}
    </svg></div><FolderLegend groups={groups} other={hasOther} /></>}
  </section>;
}
