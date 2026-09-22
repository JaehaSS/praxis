import { useEffect, useId, useMemo, useState } from "react";
import type { Task } from "../../../lib/ipc";
import { taskDotColor, taskStatusLabel } from "../../../lib/task-status";
import {
  canConnect, COLUMN_GAP, emptyProgressView, NODE_HEIGHT, NODE_WIDTH, parseProgressView,
  progressGraph, progressStorageKey, UNASSIGNED, type ProgressView,
} from "../../../lib/workflow/progress";

const button = "rounded border border-border px-2.5 py-1.5 text-xs text-text-secondary hover:bg-raised hover:text-text disabled:cursor-not-allowed disabled:opacity-40 focus-visible:outline focus-visible:outline-primary";
const input = "w-full rounded border border-border bg-surface px-2 py-1.5 text-sm text-text focus:border-primary focus:outline-none";
const title = (task: Task) => task.instruction || task.branch || `작업 #${task.id}`;

interface Props {
  tasks: Task[];
  host: string;
  projects?: string[];
  initialRepo?: string;
  loadError?: string;
  onOpenTask: (task: Task) => void;
  onRefresh?: () => void;
}

/** The existing task stream owns execution state; this view starts no work. */
export function WorkflowPanel({ tasks, host, projects = [], initialRepo, loadError, onOpenTask, onRefresh }: Props) {
  const repos = useMemo(() => [...new Set([...projects, ...tasks.filter((task) => task.host === host).map((task) => task.repo)])].sort(), [tasks, host, projects]);
  const [pickedRepo, setPickedRepo] = useState(initialRepo ?? "");
  const repo = repos.includes(pickedRepo) ? pickedRepo : repos[0] ?? "";
  return <section className="flex min-h-0 flex-1 flex-col overflow-auto p-4" aria-label="개발 진행 그래프">
    <header className="mb-3 flex flex-wrap items-start justify-between gap-3 border-b border-border pb-3">
      <div><h1 className="text-lg font-semibold">개발 진행 그래프</h1><p className="mt-1 text-sm text-text-secondary">작업의 관계와 진행 상태를 한눈에 확인합니다.</p></div>
      <div className="flex items-center gap-2"><span className="text-xs text-text-secondary">{host === "local" ? "로컬" : host}</span>{onRefresh && <button className={button} onClick={onRefresh}>새로고침</button>}</div>
    </header>
    {loadError && <p role="status" className="mb-3 text-sm text-status-awaiting">작업 목록을 갱신하지 못했습니다. {loadError}</p>}
    <label className="mb-3 flex min-w-0 items-center gap-3 text-sm">프로젝트
      <select aria-label="그래프 프로젝트" className={`${input} max-w-xl min-w-0 flex-1`} value={repo} onChange={(event) => setPickedRepo(event.target.value)} disabled={!repos.length}>
        {!repos.length && <option value="">프로젝트 없음</option>}
        {repos.map((path) => <option key={path} value={path}>{path}</option>)}
      </select>
    </label>
    {repo ? <ProjectGraph key={progressStorageKey(host, repo)} tasks={tasks} host={host} repo={repo} onOpenTask={onOpenTask} />
      : <p className="rounded border border-border p-6 text-sm text-text-secondary">작업을 만들면 여기에 진행 상태가 표시됩니다.</p>}
  </section>;
}

function loadView(key: string): { view: ProgressView; warning: string | null } {
  try {
    const text = localStorage.getItem(key);
    return { view: text ? parseProgressView(text) : emptyProgressView(), warning: null };
  } catch {
    return { view: emptyProgressView(), warning: "저장된 그래프 설정을 읽지 못했습니다. 작업 상태는 계속 확인할 수 있습니다." };
  }
}

function ProjectGraph({ tasks, host, repo, onOpenTask }: Pick<Props, "tasks" | "host" | "onOpenTask"> & { repo: string }) {
  const storageKey = progressStorageKey(host, repo);
  const [stored, setStored] = useState(() => loadView(storageKey));
  const view = stored.view;
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [predecessor, setPredecessor] = useState("");
  const [phaseName, setPhaseName] = useState("");
  const marker = `progress-arrow-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const graph = useMemo(() => progressGraph(tasks, host, repo, view), [tasks, host, repo, view]);
  const selected = graph.nodes.find((node) => node.task.id === selectedId) ?? graph.nodes[0];
  const byId = new Map(graph.nodes.map((node) => [node.task.id, node]));
  const candidates = selected ? graph.nodes.filter((node) => canConnect(graph.edges, node.task.id, selected.task.id)) : [];
  const predecessorId = candidates.some((node) => String(node.task.id) === predecessor) ? predecessor : "";

  useEffect(() => {
    const update = (event: StorageEvent) => {
      if (event.key === storageKey || event.key === null) setStored(loadView(storageKey));
    };
    window.addEventListener("storage", update);
    return () => window.removeEventListener("storage", update);
  }, [storageKey]);

  const save = (next: ProgressView) => {
    let validated: ProgressView;
    try {
      validated = parseProgressView(JSON.stringify(next));
    } catch {
      setStored({ view, warning: "그래프 설정의 크기나 형식을 확인해 주세요. 기존 설정을 유지합니다." });
      return;
    }
    try {
      localStorage.setItem(storageKey, JSON.stringify(validated));
      setStored({ view: validated, warning: null });
    } catch {
      setStored({ view: validated, warning: "그래프 설정을 저장하지 못했습니다. 변경은 이 화면을 닫기 전까지만 유지됩니다." });
    }
  };
  const choose = (id: number) => { setSelectedId(id); setPredecessor(""); setPhaseName(""); };
  const setPhase = (phase: string) => {
    if (!selected) return;
    const phaseByTask = { ...view.phaseByTask };
    if (phase) phaseByTask[String(selected.task.id)] = phase;
    else delete phaseByTask[String(selected.task.id)];
    save({ ...view, phaseByTask });
  };
  const addPhase = () => {
    const name = phaseName.trim();
    if (!selected || !name || name.length > 80 || name === UNASSIGNED || view.phases.length >= 32) return;
    save({ ...view, phases: view.phases.includes(name) ? view.phases : [...view.phases, name], phaseByTask: { ...view.phaseByTask, [selected.task.id]: name } });
    setPhaseName("");
  };
  const addDependency = () => {
    if (!selected || !predecessorId || view.dependencies.length >= 2_000) return;
    const from = Number(predecessorId);
    if (!canConnect(graph.edges, from, selected.task.id)) return;
    save({ ...view, dependencies: [...view.dependencies, { from, to: selected.task.id }] });
    setPredecessor("");
  };

  if (!graph.nodes.length) return <p className="rounded border border-border p-6 text-sm text-text-secondary">이 프로젝트에 표시할 작업이 없습니다. 기존 방식으로 작업을 만들면 여기에 자동으로 표시됩니다.</p>;
  return <>
    <div className="mb-3 flex flex-wrap gap-2" aria-label="작업 진행 요약">
      {[["전체", graph.counts.total], ["진행 중", graph.counts.running], ["확인 필요", graph.counts.awaiting], ["완료", graph.counts.done], ["실패", graph.counts.failed]].map(([label, count]) => <span key={label} className="rounded border border-border bg-surface px-2.5 py-1.5 text-sm">{label} <strong className="ml-2 font-code">{count}</strong></span>)}
    </div>
    {graph.counts.stale > 0 && <p className="mb-3 text-sm text-status-awaiting" role="status">연결이 끊긴 작업 {graph.counts.stale}개는 마지막 확인 상태입니다. 현재 진행·완료 수에서 제외합니다.</p>}
    {stored.warning && <p role="status" className="mb-3 text-sm text-status-awaiting">{stored.warning}</p>}
    {graph.omitted > 0 && <p role="status" className="mb-3 text-sm text-status-awaiting">순환되거나 중복된 연결 {graph.omitted}개는 표시하지 않았습니다.</p>}
    <div className="grid min-h-0 gap-3 xl:grid-cols-[minmax(0,1fr)_19rem]">
      <div className="min-w-0 space-y-3">
        <div className="flex flex-wrap gap-x-4 gap-y-2 text-xs text-text-secondary" aria-label="Phase별 완료 수">{[...view.phases, UNASSIGNED].map((phase) => {
          const nodes = graph.nodes.filter((node) => node.phase === phase);
          if (!nodes.length) return null;
          return <span key={phase}>{phase} <strong>{nodes.filter((node) => !node.task.stale && node.task.state === "Done").length}/{nodes.length}</strong> 완료</span>;
        })}</div>
        <div className="max-h-[28rem] min-h-48 overflow-auto rounded border border-border bg-bg p-1" aria-label="작업 관계 그래프">
          <div className="relative" style={{ width: graph.width, height: graph.height }}>
            <svg className="pointer-events-none absolute inset-0" width={graph.width} height={graph.height} aria-hidden="true">
              <defs><marker id={marker} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--c-text-muted)" /></marker></defs>
              {graph.edges.map((edge) => {
                const source = byId.get(edge.from)!; const target = byId.get(edge.to)!;
                const x1 = source.x + NODE_WIDTH; const y1 = source.y + NODE_HEIGHT / 2;
                const x2 = target.x; const y2 = target.y + NODE_HEIGHT / 2;
                return <path key={`${edge.from}:${edge.to}`} d={`M ${x1} ${y1} C ${x1 + COLUMN_GAP / 2} ${y1}, ${x2 - COLUMN_GAP / 2} ${y2}, ${x2 - 5} ${y2}`} fill="none" stroke="var(--c-text-muted)" strokeWidth="1.5" strokeDasharray={edge.kind === "continuation" ? "5 4" : undefined} markerEnd={`url(#${marker})`} />;
              })}
            </svg>
            {graph.nodes.map(({ task, phase, x, y }) => <button key={task.id} type="button" aria-label={`작업 #${task.id}: ${title(task)} · ${task.stale ? "마지막 확인 · " : ""}${taskStatusLabel(task)}`} aria-pressed={selected?.task.id === task.id} onClick={() => choose(task.id)}
              className={`absolute flex flex-col rounded-lg border bg-surface p-2.5 text-left hover:bg-raised focus-visible:outline focus-visible:outline-primary ${selected?.task.id === task.id ? "border-primary" : "border-border"}`}
              style={{ left: x, top: y, width: NODE_WIDTH, height: NODE_HEIGHT }}>
              <span className="mb-1 flex w-full justify-between gap-2 text-xs text-text-secondary"><span className="truncate">{phase}</span><span className="font-code">#{task.id}</span></span>
              <span className="line-clamp-2 w-full break-words text-sm font-medium leading-4 text-text">{title(task)}</span>
              <span className="mt-auto flex w-full items-center gap-2 text-xs text-text-secondary"><span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-full" style={{ background: task.stale ? "var(--c-text-muted)" : taskDotColor(task) }} />{task.stale ? "마지막 확인 · " : ""}{taskStatusLabel(task)}</span>
            </button>)}
          </div>
        </div>
        <p className="text-xs text-text-secondary">실선: 지정한 선행 관계 · 점선: 이전 작업에서 이어받음. Phase와 연결은 이 앱에 저장됩니다.</p>
      </div>
      {selected && <aside className="min-w-0 space-y-4 rounded border border-border bg-surface p-4" aria-label="선택한 작업 상세">
        <div><h2 className="text-sm font-semibold">작업 #{selected.task.id}</h2><p className="mt-2 whitespace-pre-wrap break-words text-sm text-text-secondary">{title(selected.task)}</p><p className="mt-2 text-xs text-text-secondary">{selected.task.stale ? "마지막 확인 · " : ""}{taskStatusLabel(selected.task)}</p></div>
        <button className={button} disabled={selected.task.stale} onClick={() => onOpenTask(selected.task)}>작업 열기</button>
        {selected.task.worktree_missing && <p className="text-xs text-status-awaiting">작업 폴더가 없어졌습니다.</p>}
        {selected.task.blocked_reason && <p className="break-words text-xs text-status-awaiting">대기 이유: {selected.task.blocked_reason}</p>}
        <label className="block space-y-1 text-xs text-text-secondary"><span>Phase</span><select aria-label="작업 Phase" className={input} value={view.phaseByTask[String(selected.task.id)] ?? ""} onChange={(event) => setPhase(event.target.value)}><option value="">{UNASSIGNED}</option>{view.phases.map((phase) => <option key={phase} value={phase}>{phase}</option>)}</select></label>
        <form className="flex gap-2" onSubmit={(event) => { event.preventDefault(); addPhase(); }}><input aria-label="새 Phase 이름" className={`${input} min-w-0`} placeholder="새 Phase 이름" maxLength={80} value={phaseName} onChange={(event) => setPhaseName(event.target.value)} /><button className={`${button} shrink-0`} disabled={!phaseName.trim() || phaseName.trim() === UNASSIGNED || view.phases.length >= 32}>추가</button></form>
        <div><h3 className="mb-2 text-xs text-text-secondary">선행 작업</h3>
          <ul className="space-y-2 text-xs">{graph.edges.filter((edge) => edge.to === selected.task.id).map((edge) => <li key={`${edge.from}:${edge.kind}`} className="flex items-start gap-2"><button className="min-w-0 flex-1 truncate text-left text-primary-bright" onClick={() => choose(edge.from)}>#{edge.from} {title(byId.get(edge.from)!.task)}</button>{edge.kind === "continuation" ? <span className="shrink-0 text-text-secondary">이어받음</span> : <button className="shrink-0 text-text-secondary hover:text-text" aria-label={`선행 작업 #${edge.from} 연결 해제`} onClick={() => save({ ...view, dependencies: view.dependencies.filter((item) => !(item.from === edge.from && item.to === edge.to)) })}>해제</button>}</li>)}</ul>
          {!graph.edges.some((edge) => edge.to === selected.task.id) && <p className="text-xs text-text-secondary">지정된 선행 작업이 없습니다.</p>}
          <div className="mt-2 flex gap-2"><select aria-label="선행 작업 선택" className={`${input} min-w-0`} value={predecessorId} onChange={(event) => setPredecessor(event.target.value)}><option value="">연결할 작업 선택</option>{candidates.map((node) => <option key={node.task.id} value={node.task.id}>#{node.task.id} {title(node.task)}</option>)}</select><button className={`${button} shrink-0`} disabled={!predecessorId || view.dependencies.length >= 2_000} onClick={addDependency}>연결</button></div>
          <p className="mt-2 text-xs text-text-secondary">진행 관계를 정리하는 표시입니다. 작업 상태는 기존 작업 화면에서 갱신됩니다.</p>
        </div>
      </aside>}
    </div>
  </>;
}
