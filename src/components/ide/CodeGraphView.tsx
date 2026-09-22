import { useId, useLayoutEffect, useRef, useState, type ReactElement } from "react";

import type { CodeGraphDirection, CodeGraphNeighborhood, CodeGraphNeighborhoodNode } from "../../lib/ipc";
import { layoutCodeGraph } from "../../lib/code-graph-layout";

interface CodeGraphViewProps {
  embedded?: boolean;
  graph: CodeGraphNeighborhood | null;
  error: string | null;
  direction: CodeGraphDirection;
  depth: number;
  onDirection: (direction: CodeGraphDirection) => void;
  onDepth: (depth: number) => void;
  onOpen: (node: CodeGraphNeighborhoodNode) => void;
  onClose: () => void;
}

const nodeLabel = (node: CodeGraphNeighborhoodNode) => `${node.name} · ${node.relPath}:${node.line + 1}`;
const shorten = (value: string) => (value.length > 22 ? `${value.slice(0, 21)}…` : value);

function Notices({ graph, error }: Pick<CodeGraphViewProps, "graph" | "error">): ReactElement | null {
  if (!graph) return <p className="text-xs text-text-muted" role={error ? "alert" : "status"}>{error ?? "참조 그래프를 불러오는 중…"}</p>;
  const edgesUnavailableEmpty = !error && graph.edgesUnavailable && graph.edges.length === 0;
  return <div className="space-y-1 text-xs text-status-awaiting">
    {error && <p role="alert">{error}</p>}
    {graph.freshness === "stale" && <p>오래된 인덱스</p>}
    {graph.edgesUnavailable && !edgesUnavailableEmpty && <p>{graph.edgesUnavailable}</p>}
    {graph.incomplete && <p>{graph.incomplete.detail}</p>}
    {graph.encounteredIncomplete.map((item) => <p key={item.relPath}>{item.relPath}: {item.reason}</p>)}
    {graph.truncated && <p>결과가 일부만 표시되었습니다.</p>}
    {!error && graph.edges.length === 0 && !graph.edgesUnavailable && !graph.incomplete && <p>저장된 참조가 없습니다. 분석 범위에 따라 누락될 수 있습니다.</p>}
  </div>;
}

function EdgesUnavailableEmptyState({ reason }: { reason: string }): ReactElement {
  return <div role="status" className="mt-2 rounded-lg border border-border bg-surface p-4 text-xs">
    <p className="text-sm text-text">이 파일은 참조를 분석하지 못했습니다</p>
    <p className="text-status-awaiting">{reason}</p>
    <p className="text-text-muted">심볼은 인덱싱됐지만 이 언어의 참조 관계는 만들지 않았습니다 — 참조가 없다는 뜻이 아닙니다.</p>
  </div>;
}

function GraphCanvas({ graph, direction, onOpen }: Pick<CodeGraphViewProps, "graph" | "direction" | "onOpen">): ReactElement | null {
  const markerId = `graph-arrow-${useId().replace(/[^a-z0-9]/gi, "")}`;
  if (!graph) return null;
  const positions = layoutCodeGraph(graph.nodes, graph.edges, graph.rootId, direction);
  const byId = new Map(positions.map((position) => [position.id, position]));
  const width = Math.max(480, ...positions.map((position) => position.x + 200));
  const height = Math.max(240, ...positions.map((position) => position.y + 52));
  return <div className="min-h-0 overflow-auto"><svg width={width} height={height} role="img" aria-label="참조 관계 그래프">
    <defs><marker id={markerId} markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="currentColor" /></marker></defs>
            {graph.edges.map((edge) => {
              const source = byId.get(edge.sourceId);
              const target = byId.get(edge.targetId);
              if (!source || !target) return null;
      const forward = source.x <= target.x;
      return <line key={`${edge.sourceId}:${edge.targetId}`} x1={source.x + (forward ? 180 : 0)} y1={source.y + 18} x2={target.x + (forward ? 0 : 180)} y2={target.y + 18} stroke="currentColor" className="text-text-muted" markerEnd={`url(#${markerId})`} />;
            })}
            {graph.nodes.map((node) => {
              const position = byId.get(node.id);
              if (!position) return null;
      return <g key={node.id} transform={`translate(${position.x} ${position.y})`} onClick={() => onOpen(node)} className="cursor-pointer"><title>{nodeLabel(node)}</title><rect width="180" height="36" rx="4" className={node.id === graph.rootId ? "fill-bg stroke-primary" : "fill-bg stroke-border"} /><text x="6" y="15" className="fill-text text-xs">{shorten(node.name)}</text><text x="6" y="29" className="fill-text-muted text-[10px]">{shorten(`${node.relPath}:${node.line + 1}`)}</text></g>;
            })}
  </svg></div>;
}

export function CodeGraphView({ embedded = false, graph, error, direction, depth, onDirection, onDepth, onOpen, onClose }: CodeGraphViewProps) {
  const panelRef = useRef<HTMLElement>(null);
  const [narrow, setNarrow] = useState(false);
  const [selectedView, setView] = useState<"list" | "graph" | null>(null);
  const view = selectedView ?? (narrow ? "list" : "graph");
  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    panel.focus();
    const measure = () => {
      const width = panel.getBoundingClientRect().width;
      if (width > 0) setNarrow(width < 600);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    return () => observer.disconnect();
  }, []);
  const root = graph?.nodes.find((node) => node.id === graph.rootId);
  const unavailableReason = graph && !error && graph.edges.length === 0 ? graph.edgesUnavailable : null;
  return <aside ref={panelRef} className={`flex min-h-0 min-w-0 flex-col bg-raised p-3 ${embedded ? "flex-1 overflow-hidden" : "absolute inset-x-2 top-10 bottom-2 z-30 rounded-lg border border-border-strong shadow-xl"}`} role="dialog" aria-label="참조 그래프" tabIndex={-1} onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); onClose(); } }}>
    <div className="mb-2 flex flex-wrap items-center gap-2 text-xs"><h2 className="text-sm text-text">참조 그래프</h2><button aria-pressed={direction === "incoming"} className={direction === "incoming" ? "text-text" : "text-text-muted"} onClick={() => onDirection("incoming")}>들어오는 참조</button><button aria-pressed={direction === "outgoing"} className={direction === "outgoing" ? "text-text" : "text-text-muted"} onClick={() => onDirection("outgoing")}>나가는 참조</button>{[1, 2, 3].map((value) => <button key={value} aria-pressed={depth === value} className={depth === value ? "text-text" : "text-text-muted"} onClick={() => onDepth(value)}>{value}단계</button>)}<button aria-pressed={view === "graph"} className={view === "graph" ? "text-text" : "text-text-muted"} onClick={() => setView("graph")}>그래프</button><button aria-pressed={view === "list"} className={view === "list" ? "text-text" : "text-text-muted"} onClick={() => setView("list")}>목록</button><button className="ml-auto text-text-muted hover:text-text" onClick={onClose} aria-label="참조 그래프 닫기">×</button></div>
    {root && <p className="mb-2 truncate text-xs text-text-muted">기준: {nodeLabel(root)}</p>}
    <Notices graph={graph} error={error} />
    {view === "graph" && (unavailableReason ? <EdgesUnavailableEmptyState reason={unavailableReason} /> : <GraphCanvas graph={graph} direction={direction} onOpen={onOpen} />)}
    {graph && <div hidden={view !== "list"} className="mt-2 min-h-0 overflow-auto" aria-label="참조 그래프 목록"><p className="text-xs text-text-muted">키보드로 열기</p>{graph.nodes.map((node) => <button key={node.id} className="block w-full truncate text-left text-xs text-text-muted hover:text-text" onClick={() => onOpen(node)}>{nodeLabel(node)}</button>)}</div>}
  </aside>;
}
