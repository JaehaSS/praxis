import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactElement,
} from "react";
import { useHostScope } from "../lib/host-scope";
import type { TaskRef } from "../lib/transport";
import { MemoryInspectorPanel } from "./ide/MemoryInspectorPanel";
import { ago, outcomeBadge } from "../lib/fmt";
import { approveMemoryWithConfirmation } from "./memory-approval";
import {
  archiveCurrentMemoryResults,
  countArchivableMemories,
} from "./memory-bulk-archive";
import { MemoryBulkArchiveAction } from "./MemoryBulkArchiveAction";
import { MemoryLifecycleGuide } from "./MemoryLifecycleGuide";
import { MemoryEvidencePanel } from "./MemoryEvidencePanel";
import {
  archiveLegacySelection,
  canArchiveMemory,
} from "./memory-migration";
import {
  confirmMessage,
  designatability,
  groupPreview,
  policyOf,
} from "./memory-application-policy";
import { memoryReadiness } from "./memory-readiness";
import { SelfImproveView } from "./SelfImproveView";
import { MemoryReadinessNote } from "./MemoryReadinessNote";
import { MemoryReviewControls } from "./MemoryReviewControls";
import { MemoryVersionPanel } from "./MemoryVersionPanel";
import { MetaTag } from "./MetaTag";
import {
  countMemoryReviewFilters,
  filterMemoryReview,
  summarizeMemoryActivation,
  type MemoryReviewFilter,
} from "./memory-review";
import {
  memoryList,
  memoryArchive,
  memoryPurge,
  memoryAdd,
  memoryUpdate,
  memoryUsages,
  memoryPreview,
  memoryConfirmAndApprove,
  memorySetApplicationPolicy,
  contextReport,
  type Memory,
  type MemoryUsageRow,
} from "../lib/ipc";

export const KINDS = [
  "claim",
  "observation",
  "decision",
  "convention",
  "abandoned",
  "pitfall",
];
const REVIEW_BATCH_SIZE = 50;

interface MemoryViewProps {
  activation?: boolean;
  preferredScope?: string;
  initialScope?: string;
  /** 처음 열 탭. 인사이트 회고에서 들어오면 자기개선으로 연다(ADR 0191). */
  initialTab?: "memory" | "selfimprove";
}

/**
 * Outcome Insights로 들어오면 검토 대상만 좁혀 연다 — 그 진입점의 목적이 검토다.
 * 그 외에는 `활성`으로 연다: 보관·거절까지 늘 보이면 정리해도 정리된 것처럼 보이지 않는다(ADR 0134).
 */
export function initialReviewFilter(activation: boolean): MemoryReviewFilter {
  return activation ? "actionable" : "active";
}

/** 프로젝트를 미리 좁히는 것도 activation 진입뿐이다. 직접 내비게이션은 전체에서 시작한다. */
export function initialReviewScope(
  activation: boolean,
  preferredScope?: string,
): string | null {
  const scope = preferredScope?.trim();
  return activation && scope ? scope : null;
}

export const kindColor: Record<string, string> = {
  claim: "text-text-secondary",
  observation: "text-status-awaiting",
  decision: "text-status-running",
  convention: "text-primary-bright",
  // abandoned = 실패로 끝난 길, pitfall = 주의해서 확인할 지점 (DESIGN.md 상태 토큰에서 선택)
  abandoned: "text-status-failed",
  pitfall: "text-status-question",
};

/** 특정 메모리의 주입(사용) 이력 — 드릴인. */
function UsageHistory({
  id,
  onInspect,
}: {
  id: number;
  onInspect: (task: TaskRef, memoryId: number) => void;
}) {
  // 메모리는 그것을 보관한 머신의 DB에 산다 — 스코프가 그 머신을 정한다 (ADR 0133).
  const host = useHostScope();
  const [state, setState] = useState<{
    host: TaskRef["host"];
    id: number;
    rows: MemoryUsageRow[] | null;
    err: string | null;
  }>({ host, id, rows: null, err: null });
  useEffect(() => {
    let active = true;
    setState({ host, id, rows: null, err: null });
    memoryUsages(host, id)
      .then((rows) => active && setState({ host, id, rows, err: null }))
      .catch((error) =>
        active && setState({ host, id, rows: null, err: String(error) }),
      );
    return () => {
      active = false;
    };
  }, [host, id]);

  if (state.host !== host || state.id !== id)
    return <div className="text-text-muted text-xs mt-2">불러오는 중…</div>;
  if (state.err)
    return <div className="text-status-failed text-xs mt-2">{state.err}</div>;
  if (!state.rows)
    return <div className="text-text-muted text-xs mt-2">불러오는 중…</div>;
  if (state.rows.length === 0)
    return (
      <div className="text-text-muted text-xs mt-2">
        기록된 사용 이력이 없습니다. 미사용 여부는 확인할 수 없습니다.
      </div>
    );

  return (
    <div className="mt-2 pt-2 border-t border-border flex flex-col gap-1">
      <div className="text-xs text-text-muted">적용된 작업 {state.rows.length}건</div>
      {state.rows.map((r, index) => {
        const b = outcomeBadge(r.outcome);
        return (
          <button
            key={`${r.task_id}-${r.injected_at}-${index}`}
            className="flex items-center gap-2 text-left text-xs"
            onClick={() => onInspect({ host, id: r.task_id }, id)}
          >
            <span className={`w-12 shrink-0 ${b.c}`}>{b.t}</span>
            <span className="text-text-secondary truncate flex-1" title={r.instruction}>
              {r.instruction || `#${r.task_id}`}
            </span>
            <span className="text-text-muted font-code shrink-0">
              {r.state} · {ago(r.injected_at)}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/** 주입 프리뷰(드라이런) — 지시문이면 어떤 메모리가 세션에 들어갈지 사전 확인. */
/** 미리보기 한 그룹. 비어 있으면 머리글도 내지 않는다 — 빈 섹션은 소음이다. */
function PreviewGroup({ label, rows }: { label: string; rows: Memory[] }): ReactElement | null {
  if (rows.length === 0) return null;
  return (
    <div className="flex flex-col gap-1">
      <div className="text-[11px] font-medium text-text-secondary">
        {label} {rows.length}건
      </div>
      {rows.map((m, i) => (
        <div key={m.id} className="flex items-start gap-2 text-xs">
          <span className="text-text-muted w-4 shrink-0">{i + 1}</span>
          <span className={`w-16 shrink-0 ${kindColor[m.kind] ?? ""}`}>{m.kind}</span>
          <span className="text-text-secondary break-words flex-1">{m.content}</span>
        </div>
      ))}
    </div>
  );
}

/**
 * 항상-적용 지정/해제 버튼.
 *
 * 구버전 Runner 응답에는 정책 필드가 없다 — 그때는 **아무것도 렌더하지 않는다**.
 * 에러로 표시하면 원격이 구버전인 것뿐인데 고장으로 보인다.
 * 지정 불가 사유는 버튼을 감추는 대신 비활성 + title로 알려, 왜 안 되는지 추측하지 않게 한다.
 */
function ApplicationPolicyAction({
  memory,
  onToggle,
}: {
  memory: Memory;
  onToggle: () => void;
}): ReactElement | null {
  if (policyOf(memory) === null) return null;
  const state = designatability(memory);
  if (state.kind === "blocked") {
    return (
      <span className="text-text-muted/50 cursor-default" title={state.reason}>
        항상 적용
      </span>
    );
  }
  const designated = state.kind === "designated";
  return (
    <button
      className={designated ? "text-primary-bright hover:text-text" : "text-text-muted hover:text-primary-bright"}
      onClick={onToggle}
    >
      {designated ? "항상 적용 해제" : "항상 적용"}
    </button>
  );
}

function PreviewTool({ repos }: { repos: string[] }) {
  // 메모리는 그것을 보관한 머신의 DB에 산다 — 스코프가 그 머신을 정한다 (ADR 0133).
  const host = useHostScope();
  const [repo, setRepo] = useState(repos[0] ?? "");
  const [instruction, setInstruction] = useState("");
  const [hits, setHits] = useState<Memory[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const run = async () => {
    setBusy(true);
    setErr(null);
    try {
      setHits(await memoryPreview(host, repo.trim(), instruction.trim()));
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="bg-surface border border-border rounded-lg p-3 mb-3">
      <div className="text-xs text-text-muted mb-2">
        적용 미리보기 — 이 지시문으로 작업을 만들면 세션에 들어갈 메모리를 미리 봅니다.
      </div>
      <div className="flex gap-2 mb-2">
        <input
          className="w-56 bg-bg border border-border rounded px-2 py-1 text-sm outline-none focus:border-primary"
          placeholder="repo 경로 (비우면 모든 레포 공통)"
          value={repo}
          onChange={(e) => setRepo(e.target.value)}
          list="mem-repos"
        />
        <datalist id="mem-repos">
          {repos.map((r) => (
            <option key={r} value={r} />
          ))}
        </datalist>
        <input
          className="flex-1 bg-bg border border-border rounded px-2 py-1 text-sm outline-none focus:border-primary"
          placeholder="지시문 예: JWT 인증 리팩터"
          value={instruction}
          onChange={(e) => setInstruction(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && run()}
        />
        <button
          className="h-7 px-3 rounded bg-primary text-bg text-sm disabled:opacity-50"
          disabled={busy}
          onClick={run}
        >
          {busy ? "…" : "미리보기"}
        </button>
      </div>
      {err && <div className="text-status-failed text-xs">{err}</div>}
      {hits &&
        (hits.length === 0 ? (
          <div className="text-text-muted text-xs">적용될 메모리가 없습니다.</div>
        ) : (
          <div className="flex flex-col gap-2">
            <div className="text-xs text-text-muted">적용 예정 {hits.length}건</div>
            <PreviewGroup label="항상 적용" rows={groupPreview(hits).mustApply} />
            <PreviewGroup label="관련 메모리" rows={groupPreview(hits).relevant} />
          </div>
        ))}
    </div>
  );
}

/** S-05 Memory View — 메모리 목록/통계 + 수동 추가·편집 + 사용 이력 + 주입 프리뷰. */
export function MemoryView({
  activation = false,
  preferredScope,
  initialScope,
  initialTab,
}: MemoryViewProps = {}): ReactElement {
  // 메모리는 그것을 보관한 머신의 DB에 산다 — 스코프가 그 머신을 정한다 (ADR 0133).
  const host = useHostScope();
  const [tab, setTab] = useState<"memory" | "selfimprove">(initialTab ?? "memory");
  const [mems, setMems] = useState<Memory[]>([]);
  const [memsHost, setMemsHost] = useState(host);
  const [err, setErr] = useState<string | null>(null);
  const [openUsage, setOpenUsage] = useState<number | null>(null);
  const [inspector, setInspector] = useState<{
    task: TaskRef;
    memoryId: number;
    report: import("../lib/ipc").ContextReport | null;
    error?: string;
  } | null>(null);
  const inspectorRequest = useRef(0);
  const listRequest = useRef(0);
  const activeHost = useRef(host);
  const scopeHost = useRef(host);
  activeHost.current = host;
  const [openEvidence, setOpenEvidence] = useState<number | null>(null);
  const [openVersions, setOpenVersions] = useState<number | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState<{ content: string; kind: string }>({ content: "", kind: "claim" });
  const [adding, setAdding] = useState(false);
  const [showPreview, setShowPreview] = useState(false);
  const [form, setForm] = useState<{ repo: string; kind: string; content: string }>({
    repo: "",
    kind: "claim",
    content: "",
  });
  const [filter, setFilter] = useState<MemoryReviewFilter>(() =>
    initialReviewFilter(activation),
  );
  const [query, setQuery] = useState("");
  const [scopeKey, setScopeKey] = useState<string | null>(() =>
    initialScope?.trim() || initialReviewScope(activation, preferredScope),
  );
  const [visibleCount, setVisibleCount] = useState(REVIEW_BATCH_SIZE);
  const [selectedLegacy, setSelectedLegacy] = useState<Set<number>>(new Set());
  const [archiveBusy, setArchiveBusy] = useState(false);

  const refresh = useCallback(async () => {
    const request = ++listRequest.current;
    const refreshHost = host;
    setErr(null);
    try {
      const next = await memoryList(refreshHost);
      if (request === listRequest.current && activeHost.current === refreshHost) {
        setMems(next);
        setMemsHost(refreshHost);
      }
    } catch (e) {
      if (request === listRequest.current && activeHost.current === refreshHost)
        setErr(String(e));
    }
  }, [host]);

  useEffect(() => {
    if (scopeHost.current !== host) {
      scopeHost.current = host;
      setScopeKey(null);
    }
    setMems([]);
    setMemsHost(host);
    setErr(null);
    setOpenUsage(null);
    setOpenEvidence(null);
    setOpenVersions(null);
    setEditing(null);
    setDraft({ content: "", kind: "claim" });
    setSelectedLegacy(new Set());
    setAdding(false);
    setForm({ repo: "", kind: "claim", content: "" });
    setArchiveBusy(false);
    inspectorRequest.current += 1;
    setInspector(null);
    void refresh();
  }, [refresh]);

  useEffect(
    () => () => {
      listRequest.current += 1;
      inspectorRequest.current += 1;
    },
    [],
  );

  const activeMems = memsHost === host ? mems : [];

  const repos = Array.from(
    new Set([
      ...activeMems.map((m) => m.scope_key).filter((s): s is string => !!s),
      ...(scopeKey === null ? [] : [scopeKey]),
    ]),
  ).sort((left, right) => left.localeCompare(right));
  const activationSummary = summarizeMemoryActivation(activeMems);
  const filterCounts = countMemoryReviewFilters(activeMems, { query, scopeKey });
  const filteredMems = filterMemoryReview(activeMems, { filter, query, scopeKey });
  const archivableFilteredCount = countArchivableMemories(filteredMems);
  const visibleMems = filteredMems.slice(0, visibleCount);
  const visibleRows = visibleMems.map((memory) => ({
    memory,
    readiness: memoryReadiness(memory),
  }));

  useEffect(() => {
    setVisibleCount(REVIEW_BATCH_SIZE);
    setSelectedLegacy(new Set());
  }, [filter, query, scopeKey]);

  const toggleLegacySelect = (id: number) => {
    setSelectedLegacy((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  };

  const archiveSelectedLegacy = async () => {
    if (selectedLegacy.size === 0) return;
    setArchiveBusy(true);
    try {
      const archived = await archiveLegacySelection(mems, selectedLegacy, {
        askConfirmation: (message) => window.confirm(message),
        archive: (id: number) => memoryArchive(host, id),
      });
      if (archived > 0) {
        setSelectedLegacy(new Set());
        await refresh();
      }
    } catch (e) {
      setSelectedLegacy(new Set());
      await refresh();
      setErr(String(e));
    } finally {
      setArchiveBusy(false);
    }
  };

  const archiveFilteredMemories = async (): Promise<void> => {
    setArchiveBusy(true);
    setErr(null);
    try {
      const archived = await archiveCurrentMemoryResults(filteredMems, {
        askConfirmation: (message) => window.confirm(message),
        archive: (id: number) => memoryArchive(host, id),
      });
      if (archived > 0) await refresh();
    } catch (e) {
      await refresh();
      setErr(String(e));
    } finally {
      setArchiveBusy(false);
    }
  };

  const archiveOne = async (id: number) => {
    const confirmed = window.confirm(
      "이 메모리를 보관할까요? 본문·근거·사용 이력은 유지됩니다.",
    );
    if (!confirmed) return;
    try {
      await memoryArchive(host, id);
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  /**
   * 보관된 메모리의 영구 삭제. `보관됨` 필터에서만 닿을 수 있고 되돌릴 수 없다.
   * 확인 문구는 남는 것이 아니라 **사라지는 것**을 말한다 — 보관과 헷갈리면 복구할 방법이 없다.
   */
  const purgeOne = async (id: number) => {
    const confirmed = window.confirm(
      "이 메모리를 영구 삭제할까요? 본문이 DB에서 사라지고 되돌릴 수 없습니다.\n" +
        "누가 언제 삭제했는지 감사 이력만 남습니다.",
    );
    if (!confirmed) return;
    try {
      await memoryPurge(host, id);
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  /** 항상-적용 지정/해제. 확인을 취소하면 요청을 **한 건도** 보내지 않는다. */
  const toggleApplicationPolicy = async (m: Memory) => {
    const current = policyOf(m);
    if (current === null) return; // 구버전 Runner — 제어 자체가 없다.
    if (!window.confirm(confirmMessage(m))) return;
    const next = current === "must_apply" ? "relevance" : "must_apply";
    try {
      await memorySetApplicationPolicy(host, m.id, next, m.current_version, current);
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const saveAdd = async () => {
    if (!form.content.trim()) return;
    try {
      await memoryAdd(host, form.repo, form.kind, form.content);
      setForm({ repo: form.repo, kind: form.kind, content: "" });
      setAdding(false);
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const saveEdit = async (id: number) => {
    if (!draft.content.trim()) return;
    try {
      await memoryUpdate(host, id, draft.content, draft.kind);
      setEditing(null);
      await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const confirmAndApprove = async (memory: Memory): Promise<void> => {
    try {
      const changed = await approveMemoryWithConfirmation(memory, {
        askConfirmation: (message) => window.confirm(message),
        confirmAndApprove: (id: number, expectedVersion: number) =>
          memoryConfirmAndApprove(host, id, expectedVersion),
      });
      if (changed) await refresh();
    } catch (e) {
      setErr(String(e));
    }
  };

  const openMemoryCandidates = async (): Promise<void> => {
    await refresh();
    setFilter("candidate");
    setTab("memory");
  };

  const inspectUsage = (task: TaskRef, memoryId: number): void => {
    const request = ++inspectorRequest.current;
    setInspector({ task, memoryId, report: null });
    void contextReport(task)
      .then((report) => {
        if (request === inspectorRequest.current && activeHost.current === task.host)
          setInspector({ task, memoryId, report });
      })
      .catch((error) => {
        if (request === inspectorRequest.current && activeHost.current === task.host)
          setInspector({ task, memoryId, report: null, error: String(error) });
      });
  };

  const tabs: { key: typeof tab; label: string }[] = [
    { key: "memory", label: "메모리" },
    { key: "selfimprove", label: "자기개선" },
  ];

  // roving tabIndex는 화살표 이동과 세트다 — 핸들러 없이 tabIndex만 -1로 두면
  // 선택되지 않은 탭에 키보드로 닿을 방법이 사라진다.
  const onTabKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const current = tabs.findIndex((t) => t.key === tab);
    const last = tabs.length - 1;
    const next =
      event.key === "ArrowRight" ? (current + 1) % tabs.length
      : event.key === "ArrowLeft" ? (current + last) % tabs.length
      : event.key === "Home" ? 0
      : event.key === "End" ? last
      : -1;
    if (next < 0) return;
    event.preventDefault();
    setTab(tabs[next].key);
    event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  };

  return (
    <div className="flex-1 min-h-0 flex flex-col">
      {/* 면을 바꾸는 컨트롤이므로 Tabs 문법 — 범위를 좁히는 상태 필터(FilterSegment)와 구별한다. */}
      <div className="border-b border-border px-4">
        <div
          role="tablist"
          aria-label="메모리 화면"
          onKeyDown={onTabKey}
          className="max-w-3xl mx-auto flex items-center gap-4"
        >
          {tabs.map((t) => (
            <button
              key={t.key}
              role="tab"
              type="button"
              aria-selected={tab === t.key}
              tabIndex={tab === t.key ? 0 : -1}
              onClick={() => setTab(t.key)}
              className={`h-9 -mb-px border-b-2 text-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary ${
                tab === t.key
                  ? "border-primary text-primary-bright font-medium"
                  : "border-transparent text-text-secondary hover:text-text"
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="max-w-3xl mx-auto">
          <MemoryLifecycleGuide />
        </div>
      </div>
      {tab === "selfimprove" ? (
        <SelfImproveView onOpenMemoryCandidates={openMemoryCandidates} />
      ) : (
    <div className="flex-1 overflow-auto p-4">
      {err && <div className="text-status-failed text-sm font-code mb-2 max-w-3xl mx-auto">{err}</div>}
      <div className="max-w-3xl mx-auto flex items-center gap-2 mb-3 flex-wrap">
        <button
          className="h-8 px-3 rounded-md bg-primary text-bg text-sm disabled:opacity-50"
          onClick={() => setAdding((v) => !v)}
        >
          + 메모리 추가
        </button>
        <button
          className="h-8 px-3 rounded-md bg-surface border border-border text-text-secondary text-sm hover:border-border-strong"
          onClick={() => setShowPreview((v) => !v)}
        >
          적용 미리보기
        </button>
        <MemoryBulkArchiveAction
          count={archivableFilteredCount}
          busy={archiveBusy}
          onArchive={() => void archiveFilteredMemories()}
        />
        {selectedLegacy.size > 0 && (
          <button
            className="h-8 px-3 rounded-md bg-status-awaiting/15 border border-border text-status-awaiting text-sm disabled:opacity-50"
            disabled={archiveBusy}
            onClick={archiveSelectedLegacy}
          >
            {archiveBusy
              ? "보관 중…"
              : `이관 제외·선택 보관 (${selectedLegacy.size})`}
          </button>
        )}
      </div>

      <div className="max-w-3xl mx-auto">
        <MemoryReviewControls
          activation={activation}
          summary={activationSummary}
          query={query}
          filter={filter}
          scopeKey={scopeKey}
          scopes={repos}
          filterCounts={filterCounts}
          filteredCount={filteredMems.length}
          totalCount={activeMems.length}
          visibleCount={Math.min(visibleCount, filteredMems.length)}
          onQueryChange={setQuery}
          onFilterChange={setFilter}
          onScopeChange={setScopeKey}
        />
        {showPreview && <PreviewTool repos={repos} />}

        {adding && (
          <div className="bg-surface border border-border rounded-lg p-3 mb-3 flex flex-col gap-2">
            <div className="flex gap-2">
              <input
                className="w-56 bg-bg border border-border rounded px-2 py-1 text-sm outline-none focus:border-primary"
                placeholder="repo 경로 (비우면 모든 레포 공통)"
                value={form.repo}
                onChange={(e) => setForm({ ...form, repo: e.target.value })}
                list="mem-repos-add"
              />
              <datalist id="mem-repos-add">
                {repos.map((r) => (
                  <option key={r} value={r} />
                ))}
              </datalist>
              <select
                className="bg-bg border border-border rounded px-2 py-1 text-sm outline-none focus:border-primary"
                value={form.kind}
                onChange={(e) => setForm({ ...form, kind: e.target.value })}
              >
                {KINDS.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
            </div>
            <textarea
              className="bg-bg border border-border rounded px-2 py-1 text-sm outline-none focus:border-primary resize-y min-h-[3rem]"
              placeholder="지식 후보 내용 — 증거와 사람 승인 뒤에만 관련 작업에 적용됩니다."
              value={form.content}
              onChange={(e) => setForm({ ...form, content: e.target.value })}
            />
            <div className="flex gap-2">
              <button
                className="h-7 px-3 rounded bg-primary text-bg text-sm disabled:opacity-50"
                disabled={!form.content.trim()}
                onClick={saveAdd}
              >
                저장
              </button>
              <button
                className="h-7 px-3 rounded text-text-secondary text-sm hover:text-text"
                onClick={() => setAdding(false)}
              >
                취소
              </button>
            </div>
          </div>
        )}

        {mems.length === 0 && !adding ? (
          <div className="h-full flex items-center justify-center text-text-muted text-center px-8 py-16">
            아직 메모리가 없습니다.
            <br />
            작업을 완료하면 세션에서 프로젝트 메모리가 자동으로 기록되며, 위에서 직접 추가할 수도 있습니다.
          </div>
        ) : filteredMems.length === 0 ? (
          <div className="text-text-muted text-center px-8 py-16">
            이 필터에 해당하는 메모리가 없습니다.
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {visibleRows.map(({ memory: m, readiness }) => (
              <div key={m.id} className="bg-surface border border-border rounded-lg p-3">
                {editing === m.id ? (
                  <div className="flex flex-col gap-2">
                    <div className="flex gap-2">
                      <select
                        className="bg-bg border border-border rounded px-2 py-1 text-sm outline-none focus:border-primary"
                        value={draft.kind}
                        onChange={(e) => setDraft({ ...draft, kind: e.target.value })}
                      >
                        {KINDS.map((k) => (
                          <option key={k} value={k}>
                            {k}
                          </option>
                        ))}
                      </select>
                      <span className="text-text-muted text-xs self-center">
                        {m.scope_key ?? "모든 레포 공통"} · {m.tier}
                      </span>
                    </div>
                    <textarea
                      className="bg-bg border border-border rounded px-2 py-1 text-sm outline-none focus:border-primary resize-y min-h-[3rem]"
                      value={draft.content}
                      onChange={(e) => setDraft({ ...draft, content: e.target.value })}
                    />
                    <div className="flex gap-2">
                      <button
                        className="h-7 px-3 rounded bg-primary text-bg text-sm disabled:opacity-50"
                        disabled={!draft.content.trim()}
                        onClick={() => saveEdit(m.id)}
                      >
                        저장
                      </button>
                      <button
                        className="h-7 px-3 rounded text-text-secondary text-sm hover:text-text"
                        onClick={() => setEditing(null)}
                      >
                        취소
                      </button>
                    </div>
                  </div>
                ) : (
                  <>
                    <div className="flex items-start gap-3">
                      {m.status === "legacy_unverified" && (
                        <input
                          type="checkbox"
                          aria-label={`legacy memory #${m.id} 선택`}
                          className="mt-1 shrink-0"
                          checked={selectedLegacy.has(m.id)}
                          onChange={() => toggleLegacySelect(m.id)}
                        />
                      )}
                      <span
                        className={`text-xs font-medium mt-0.5 w-20 shrink-0 ${kindColor[m.kind] ?? "text-text-secondary"}`}
                      >
                        {m.knowledge_type}
                      </span>
                      <div className="flex-1 min-w-0">
                        <div className="text-md break-words">{m.content}</div>
                        <div className="flex items-center gap-2 mt-1.5">
                          {m.dormant && <MetaTag>휴면</MetaTag>}
                          <MetaTag>{m.status}</MetaTag>
                          {policyOf(m) === "must_apply" && (
                            <MetaTag
                              tone="accent"
                              title="검색 순위와 무관하게 모든 새 작업에 투영됩니다"
                            >
                              항상 적용
                            </MetaTag>
                          )}
                          <span className="text-text-muted text-xs font-code truncate">
                            {m.scope_key ?? "모든 레포 공통"} · 적용 {m.usage_count}회
                            {m.last_used != null && ` · ${ago(m.last_used)} 전`}
                          </span>
                        </div>
                        <MemoryReadinessNote readiness={readiness} />
                      </div>
                      <div className="flex flex-col items-end gap-1 shrink-0 text-xs">
                        <button
                          className="text-text-muted hover:text-text"
                          onClick={() =>
                            setOpenUsage((cur) => (cur === m.id ? null : m.id))
                          }
                        >
                          사용이력{m.usage_count > 0 ? ` (${m.usage_count})` : ""}
                        </button>
                        <button
                          className="text-text-muted hover:text-text"
                          onClick={() => setOpenEvidence((cur) => (cur === m.id ? null : m.id))}
                        >
                          근거
                        </button>
                        <button
                          className="text-text-muted hover:text-text"
                          onClick={() => setOpenVersions((cur) => (cur === m.id ? null : m.id))}
                        >
                          버전 v{m.current_version}
                        </button>
                        <button
                          className="text-text-muted hover:text-text"
                          onClick={() => {
                            setEditing(m.id);
                            setDraft({ content: m.content, kind: m.knowledge_type });
                          }}
                        >
                          편집
                        </button>
                        {readiness.canApprove && (
                          <button
                            className="text-primary-bright hover:text-text"
                            onClick={() => void confirmAndApprove(m)}
                          >
                            직접 확인 후 승인
                          </button>
                        )}
                        <ApplicationPolicyAction
                          memory={m}
                          onToggle={() => void toggleApplicationPolicy(m)}
                        />
                        {canArchiveMemory(m.status) && (
                          <button
                            className="text-text-muted hover:text-status-awaiting"
                            onClick={() => void archiveOne(m.id)}
                          >
                            보관
                          </button>
                        )}
                        {filter === "archived" && m.status === "archived" && (
                          <button
                            className="text-text-muted hover:text-status-failed"
                            onClick={() => void purgeOne(m.id)}
                          >
                            영구 삭제
                          </button>
                        )}
                      </div>
                    </div>
                    {openUsage === m.id && <UsageHistory id={m.id} onInspect={inspectUsage} />}
                    {openEvidence === m.id && (
                      <MemoryEvidencePanel memory={m} onChanged={refresh} />
                    )}
                    {openVersions === m.id && (
                      <MemoryVersionPanel memory={m} onChanged={refresh} />
                    )}
                  </>
                )}
              </div>
            ))}
            {visibleCount < filteredMems.length && (
              <button
                type="button"
                className="h-8 rounded-md border border-border bg-surface px-3 text-sm text-text-secondary hover:border-border-strong hover:text-text"
                onClick={() => setVisibleCount((count) => count + REVIEW_BATCH_SIZE)}
              >
                다음 {Math.min(REVIEW_BATCH_SIZE, filteredMems.length - visibleCount)}건 더 보기
              </button>
            )}
          </div>
        )}
      </div>
      {inspector?.task.host === host && inspector.report && (
        <MemoryInspectorPanel
          task={inspector.task}
          report={inspector.report}
          selectedMemoryId={inspector.memoryId}
          onClose={() => {
            inspectorRequest.current += 1;
            setInspector(null);
          }}
        />
      )}
      {inspector?.task.host === host && !inspector.report && !inspector.error && (
        <div role="status" className="text-text-muted text-xs mt-2">
          작업 컨텍스트를 불러오는 중…
        </div>
      )}
      {inspector?.task.host === host && inspector.error && (
        <div className="text-status-failed text-xs mt-2">
          작업 컨텍스트를 읽지 못했습니다: {inspector.error}
        </div>
      )}
    </div>
      )}
    </div>
  );
}
