import { useEffect, useState } from "react";
import type { FileDiff, Task } from "../lib/ipc";
import { ago } from "../lib/fmt";
import { badgeLabelFor } from "../lib/agents";
import { api } from "./api";
import { DiffPane } from "./DiffPane";
import { BottomSheet, Button, Empty, Spinner, StatusPill, StatusStrip } from "./primitives";
import { TerminalTab } from "./TerminalTab";
import { ConvoTab } from "./ConvoTab";
import { FilesTab } from "./FilesTab";
import { availableActions, followupAvailability, type ActionSpec } from "./actions";
import { isDirectRun } from "../components/ide/discard-confirm";
import { taskStateLabel } from "./status";
import { linkProps } from "./router";
import { defaultTabFor, TASK_TABS, taskHref, type TaskTab } from "./routes";

const TAB_LABEL: Record<TaskTab, string> = {
  review: "리뷰",
  convo: "대화",
  terminal: "터미널",
  files: "파일",
};

function Tabs({ id, active }: { id: number; active: TaskTab }) {
  return (
    <nav className="flex border-b border-border">
      {TASK_TABS.map((tab) => (
        <a
          key={tab}
          {...linkProps(taskHref(id, tab))}
          aria-current={tab === active ? "page" : undefined}
          // active = 하단선 2px primary + text primaryBright (DESIGN.md Tabs).
          className={`min-h-[44px] flex-1 border-b-2 pt-3 text-center text-sm ${
            tab === active
              ? "border-primary font-medium text-primary-bright"
              : "border-transparent text-text-muted"
          }`}
        >
          {TAB_LABEL[tab]}
        </a>
      ))}
    </nav>
  );
}

function Header({ task }: { task: Task }) {
  const state = taskStateLabel(task.state, task.awaiting_kind);
  const badge = badgeLabelFor(task.agent ?? null);
  return (
    <div className="relative space-y-2 border-b border-border py-3 pl-5 pr-4">
      <StatusStrip tone={state.tone} />
      <div className="flex items-center gap-2">
        <StatusPill tone={state.tone}>{state.label}</StatusPill>
        <span className="ml-auto font-code text-xs text-text-muted">
          {ago(task.updated_at)} 전
        </span>
      </div>
      <div className="text-md text-text">{task.instruction}</div>
      {/* 경로·Task ID는 code 레지스터 (DESIGN.md Do #3). */}
      <div className="truncate font-code text-xs text-text-muted">
        #{task.id} · {task.repo}
        {badge ? ` · ${badge}` : ""}
      </div>
    </div>
  );
}

function ReviewTab({ id }: { id: number }) {
  const [files, setFiles] = useState<FileDiff[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setFiles(null);
    setError(null);
    api
      .taskDiff(id)
      .then((result) => {
        // 모바일은 범위를 고르지 않는다 — 서버 기본값(세션 전체)을 그대로 본다.
        if (!cancelled) setFiles(result.files);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [id]);

  if (error) return <Empty>diff를 불러오지 못했습니다. {error}</Empty>;
  if (!files) return <Spinner label="변경 내용을 불러오는 중" />;
  return <DiffPane files={files} />;
}

export function TaskDetailScreen({ id, tab }: { id: number; tab: TaskTab | null }) {
  const [task, setTask] = useState<Task | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [pendingAction, setPendingAction] = useState<ActionSpec | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    api
      .taskList()
      .then((tasks) => {
        if (cancelled) return;
        const found = tasks.find((candidate) => candidate.id === id);
        if (found) setTask(found);
        else setError("작업을 찾을 수 없습니다.");
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [id, revision]);

  // 상태가 바뀌면 다시 읽는다 — 대화 턴이 끝나 AwaitingReview가 되는 순간 전송 버튼이
  // 살아나야 하고, 새로고침을 눌러야 알 수 있으면 대화가 끊긴다.
  useEffect(() => {
    const stop = api.subscribeEvents(
      0,
      (event) => {
        if (event.task_id === id && event.kind !== "output") {
          setRevision((value) => value + 1);
        }
      },
      () => {},
    );
    return stop;
  }, [id]);

  const run = async (action: ActionSpec) => {
    setBusy(true);
    setActionError(null);
    try {
      if (action.kind === "approve") await api.taskApprove(id);
      else await api.taskDiscard(id);
      setPendingAction(null);
      // 상태는 서버가 정한다 — 낙관적으로 그리지 않고 다시 읽는다.
      setRevision((value) => value + 1);
    } catch (cause: unknown) {
      // 실패 사유를 그대로 보여준다. "대화가 살아있어 승인할 수 없다" 같은 가드가
      // 이유 없이 삼켜지면 폰에서는 원인을 알 방법이 없다.
      setActionError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  if (error) return <Empty>{error}</Empty>;
  if (!task) return <Spinner label="작업을 불러오는 중" />;

  const isDirect = isDirectRun(task);
  const actions = availableActions(task).map((action) => {
    if (action.kind !== "approve") return action;
    if (isDirect) return { ...action, label: "승인", confirm: "변경을 승인합니다." };
    return {
      ...action,
      label: `${task.base}에 승인하고 머지`,
      confirm: `변경을 ${task.base} 브랜치에 머지합니다. 되돌리려면 git에서 직접 되돌려야 합니다.`,
    };
  });
  // 탭이 지정되지 않았으면 작업 mode가 정한다 — 대화 작업을 열었는데 diff가 뜨면
  // 들어온 목적과 어긋난다.
  const active = tab ?? defaultTabFor(task.mode);
  const followup = followupAvailability(task);

  return (
    <div>
      <Header task={task} />
      <Tabs id={id} active={active} />
      {active === "review" ? (
        <ReviewTab id={id} />
      ) : active === "terminal" ? (
        // 터미널만 고정 높이를 갖는다 — 자체 스크롤로 바닥 고정을 판단해야 하기 때문.
        <div className="h-[60vh]">
          <TerminalTab id={id} />
        </div>
      ) : active === "convo" ? (
        <ConvoTab id={id} canSend={followup.canSend} blockedReason={followup.reason} />
      ) : (
        <FilesTab id={id} />
      )}

      {actionError ? (
        <div className="border-t border-dangerborder bg-dangerbg px-4 py-3 text-sm text-text">
          {actionError}
        </div>
      ) : null}

      {actions.length > 0 ? (
        <div
          className="sticky bottom-0 flex gap-2 border-t border-border bg-surface px-4 py-3"
          style={{ paddingBottom: "calc(env(safe-area-inset-bottom) + 0.75rem)" }}
        >
          {actions.map((action) => (
            <div key={action.kind} className="min-w-0 flex-1">
              <Button variant={action.variant} onClick={() => setPendingAction(action)}>
                <span className="block truncate" title={action.label}>{action.label}</span>
              </Button>
            </div>
          ))}
        </div>
      ) : null}

      <BottomSheet
        open={pendingAction !== null}
        title={pendingAction?.label ?? ""}
        onClose={() => (busy ? undefined : setPendingAction(null))}
      >
        <div className="space-y-3">
          <p className="text-sm text-text-secondary">{pendingAction?.confirm}</p>
          <Button
            variant={pendingAction?.variant}
            disabled={busy}
            onClick={() => pendingAction && void run(pendingAction)}
          >
            {busy ? "처리 중…" : `${pendingAction?.label} 확인`}
          </Button>
          <Button disabled={busy} onClick={() => setPendingAction(null)}>
            취소
          </Button>
        </div>
      </BottomSheet>
    </div>
  );
}
