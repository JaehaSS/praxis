import { useState } from "react";
import { Icon, type IconName } from "./icons";
import {
  SessionTaskNavigation,
  type SessionTaskNavigationProps,
} from "./SessionTaskNavigation";

export type View =
  | "home"
  | "workspace"
  | "ensemble"
  | "workflow"
  | "insights"
  | "wiki"
  | "settings";

import type { HostFailure } from "../../lib/task-list-merge";
import { LOCAL_HOST, listHosts, type HostId } from "../../lib/transport";

/** 로컬은 이름을 그대로 쓰면 무슨 뜻인지 모른다. 원격은 프로필 이름이 곧 라벨이다. */
const hostLabel = (host: HostId): string => (host === LOCAL_HOST ? "로컬" : host);

interface Props extends SessionTaskNavigationProps {
  view: View;
  /** 응답하지 않은 호스트 — 목록은 나머지로 그리되 빠진 이유를 남긴다. */
  hostFailures?: HostFailure[];
  /** 읽지 않은 주간 회고가 있는가 — 인사이트 항목의 신선도 점(설계 0054 DR-6). */
  retroUnread?: boolean;
  /**
   * 카드의 "다시 연결" — 목록 재조회가 아니라 그 호스트의 터널을 다시 연다. 저장만 된
   * 프로필은 재조회로는 영영 살아나지 않는다. 끝날 때까지 카드가 "연결 중"으로 잠긴다.
   */
  onRetryHost?: (host: HostId) => Promise<void> | void;
  onNewTask: () => void;
  onQuickLink: (v: View) => void;
  /**
   * 세션에 속하지 않는 화면(파일·메모리·일정·GitHub·Quick Open)이 보는 호스트.
   * 세션은 각자 호스트를 갖지만 이 화면들은 그렇지 않다 — 어느 머신을 보는지
   * **화면에 적어 두고 사용자가 고르게** 한다. 숨은 전역으로 두면 마지막 연결이
   * 조용히 결정한다 (ADR 0133 결정 1).
   */
  browsingHost: HostId;
  onPickBrowsingHost: (host: HostId) => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
}

const QUICK: { v: View; icon: IconName; label: string }[] = [
  { v: "workflow", icon: "branch", label: "작업 그래프" },
  { v: "insights", icon: "chart", label: "인사이트" },
  { v: "wiki", icon: "markdown", label: "Wiki" },
];

/** 원격 탐색 중 Wiki가 잠기는 이유 — 사이드바 툴팁과 본문 안내가 같은 문장을 쓴다. */
export const WIKI_LOCAL_ONLY_REASON = "창고는 이 컴퓨터의 폴더라 로컬 호스트에서만 열립니다.";

const ORCH_VIEWS_STORAGE_KEY = "praxis-orch-views-open";

function readOrchViewsOpen(): boolean {
  try {
    return typeof window === "undefined"
      || window.localStorage.getItem(ORCH_VIEWS_STORAGE_KEY) !== "0";
  } catch {
    return true;
  }
}

function persistOrchViewsOpen(open: boolean): void {
  try {
    window.localStorage.setItem(ORCH_VIEWS_STORAGE_KEY, open ? "1" : "0");
  } catch {
    // 저장소가 차단된 환경에서도 현재 세션의 접힘 동작은 유지한다.
  }
}

/** Claude-desktop식 단일 사이드바: 새 작업 + 빠른 링크 + 푸터. */
export function Sidebar({
  view,
  onNewTask,
  onQuickLink,
  browsingHost,
  onPickBrowsingHost,
  collapsed,
  onToggleCollapse,
  tasks,
  selectedKey,
  projects,
  onOpenTask,
  onNewInRepo,
  onDeleteTask,
  onRemoveProject,
  onDiscardOrphans,
  groups,
  onProjectGroupsChange,
  hostFailures = [],
  onRetryHost,
  retroUnread = false,
}: Props) {
  const hosts = listHosts();
  const [retryingHosts, setRetryingHosts] = useState<ReadonlySet<HostId>>(() => new Set());
  const retryHost = async (host: HostId): Promise<void> => {
    if (!onRetryHost || retryingHosts.has(host)) return;
    setRetryingHosts((prev) => new Set(prev).add(host));
    try {
      await onRetryHost(host);
    } catch {
      // 실패는 카드 문구가 말한다 — 다음 재조회가 "응답 없음"을 그대로 두면 그것이 답이다.
    } finally {
      setRetryingHosts((prev) => {
        const next = new Set(prev);
        next.delete(host);
        return next;
      });
    }
  };
  // Orchestrator Views 섹션 접힘 상태 — localStorage 영속 (기본: 펼침).
  const [orchViewsOpen, setOrchViewsOpen] = useState(readOrchViewsOpen);
  const toggleOrchViews = (): void => {
    setOrchViewsOpen((open) => {
      const next = !open;
      persistOrchViewsOpen(next);
      return next;
    });
  };

  // 접힘: 얇은 레일 — 펼치기 + 새 작업 + 설정만.
  if (collapsed) {
    return (
      <aside className="w-10 shrink-0 bg-surface border-r border-border flex flex-col items-center min-h-0 py-2 gap-1">
        <button
          onClick={onToggleCollapse}
          className="p-1.5 text-text-muted hover:text-text"
          title="사이드바 펼치기 (⌥⌘B)"
          aria-label="사이드바 펼치기"
        >
          <Icon name="chevronRight" size={16} />
        </button>
        <button
          onClick={onNewTask}
          className="p-1.5 text-text-secondary hover:text-text"
          title="새 작업"
          aria-label="새 작업"
        >
          <Icon name="plus" size={16} />
        </button>
        <button
          onClick={() => onQuickLink("settings")}
          className="mt-auto p-1.5 text-text-muted hover:text-text"
          title="설정"
          aria-label="설정"
        >
          <Icon name="settings" size={16} />
        </button>
      </aside>
    );
  }

  return (
    <aside className="w-60 shrink-0 bg-surface border-r border-border flex flex-col min-h-0">
      <div className="p-2.5 pb-1">
        <div className="flex items-center gap-1.5">
          <button
            onClick={onNewTask}
            className={`flex-1 min-w-0 flex items-center gap-2 text-sm font-medium px-2.5 py-2 rounded-md border ${
              view === "home" ? "border-border-strong text-text" : "border-border text-text-secondary hover:border-border-strong"
            }`}
          >
            <Icon name="plus" size={16} /> 새 작업
          </button>
          <button
            onClick={onToggleCollapse}
            className="p-1.5 shrink-0 text-text-muted hover:text-text"
            title="사이드바 접기 (⌥⌘B)"
            aria-label="사이드바 접기"
          >
            <Icon name="chevronLeft" size={16} />
          </button>
        </div>
      </div>

      <div className="px-1.5 py-1.5">
        <button
          onClick={toggleOrchViews}
          className="w-full flex items-center gap-1 px-2.5 py-1 text-[11px] font-semibold text-text-muted hover:text-text uppercase tracking-wider"
          aria-expanded={orchViewsOpen}
        >
          Orchestrator Views
          <Icon name={orchViewsOpen ? "chevronDown" : "chevronRight"} size={12} />
        </button>
        {orchViewsOpen && (
          <div className="flex flex-col gap-0.5 px-1">
            {QUICK.map((q) => {
              /* 창고는 이 컴퓨터의 폴더라 원격 탐색 중에는 열리지 않는다. 그렇다고 항목을
                 지워 버리면 **사라진 이유가 화면 어디에도 남지 않는다** — 본문의 안내는
                 진입점이 없어 도달할 수 없고, 사용자는 기능이 없어졌다고 읽는다.
                 자리는 지키고 잠긴 이유를 붙인다. */
              const localOnly = q.v === "wiki" && browsingHost !== LOCAL_HOST;
              return (
                <button
                  key={q.v}
                  onClick={() => onQuickLink(q.v)}
                  disabled={localOnly}
                  title={localOnly ? WIKI_LOCAL_ONLY_REASON : undefined}
                  className={`flex items-center gap-2 px-2.5 py-1.5 rounded-md text-sm ${
                    localOnly
                      ? "text-text-muted cursor-not-allowed"
                      : view === q.v
                        ? "bg-raised text-text"
                        : "text-text-secondary hover:text-text"
                  }`}
                >
                  <Icon name={q.icon} size={16} /> {q.label}
                  {localOnly && (
                    <span className="ml-auto shrink-0 text-[10px] text-text-muted">로컬 전용</span>
                  )}
                  {/* 적체 건수가 아니라 **새 다이제스트**일 때만 켠다. 만성적으로 켜져 있는
                      배지는 두 주면 무시된다(설계 0054 DR-6). */}
                  {q.v === "insights" && retroUnread && (
                    <span
                      className="ml-auto w-1.5 h-1.5 rounded-full bg-primary-bright shrink-0"
                      title="읽지 않은 주간 회고"
                      aria-label="읽지 않은 주간 회고"
                    />
                  )}
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* Orchestrator Views 아래 상시 노출: 세션 작업 네비게이션 (Codex 데스크탑식) */}
      <div className="flex-1 overflow-auto px-1.5 pt-1 border-t border-border">
        {/* 응답 없는 호스트는 목록에서 빠지되 사라지지는 않는다 — 목록이 짧아진 이유가
            "작업이 없어서"인지 "호스트가 죽어서"인지 구분되어야 한다. */}
        {hostFailures.map((failure) => {
          const retrying = retryingHosts.has(failure.host);
          return (
            <div
              key={failure.host}
              className="mx-1 mb-1.5 flex items-center justify-between gap-2 rounded-md border border-dangerborder bg-dangerbg px-2 py-1.5 text-[11px]"
              data-host-failure={failure.host}
            >
              <span className="truncate text-status-failed" title={failure.error}>
                {hostLabel(failure.host)} — {retrying ? "연결 중…" : "응답 없음"}
              </span>
              {onRetryHost && (
                <button
                  onClick={() => void retryHost(failure.host)}
                  disabled={retrying}
                  className="shrink-0 rounded bg-raised px-1.5 py-0.5 text-text-secondary hover:text-text disabled:opacity-50"
                >
                  {failure.host === LOCAL_HOST ? "다시 시도" : "다시 연결"}
                </button>
              )}
            </div>
          );
        })}
        {/* 최근 세션 평면 목록은 `SessionTaskNavigation`이 트리 위에 함께 그린다 — 행 우클릭 메뉴를
            트리 카드와 공유하기 위해서다. */}
        <SessionTaskNavigation
          tasks={tasks}
          selectedKey={selectedKey}
          projects={projects}
          onOpenTask={onOpenTask}
          onNewInRepo={onNewInRepo}
          onDeleteTask={onDeleteTask}
          onRemoveProject={onRemoveProject}
          onDiscardOrphans={onDiscardOrphans}
          groups={groups}
          onProjectGroupsChange={onProjectGroupsChange}
        />
      </div>

      <div className="p-2.5 border-t border-border flex items-center gap-2">
        <span className="w-6 h-6 rounded-full bg-raised text-primary-bright flex items-center justify-center text-xs shrink-0 font-bold">
          P
        </span>
        {/* 호스트가 하나뿐이면 고를 것이 없다 — 손잡이 대신 사실만 적는다. */}
        {hosts.length <= 1 ? (
          <span className="flex-1 min-w-0 truncate text-sm text-text-secondary">
            Praxis · {hostLabel(browsingHost)}
          </span>
        ) : (
          <label
            className="flex min-w-0 flex-1 items-center gap-1 text-sm text-text-secondary"
            title="파일·메모리·일정·GitHub 화면이 볼 호스트"
          >
            <span className="shrink-0">Praxis ·</span>
            <select
              aria-label="탐색 호스트"
              className={`min-w-0 flex-1 bg-transparent outline-none ${
                browsingHost === LOCAL_HOST ? "" : "text-primary-bright"
              }`}
              value={browsingHost}
              onChange={(event) => onPickBrowsingHost(event.target.value)}
            >
              {hosts.map((host) => (
                <option key={host} value={host} className="bg-surface text-text">
                  {hostLabel(host)}
                </option>
              ))}
            </select>
          </label>
        )}
        <button
          onClick={() => onQuickLink("settings")}
          className="text-text-muted hover:text-text shrink-0"
          aria-label="설정"
          title="설정"
        >
          <Icon name="settings" size={16} />
        </button>
      </div>
    </aside>
  );
}
