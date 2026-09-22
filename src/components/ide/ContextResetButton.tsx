import { useState, type ReactElement } from "react";
import { convoContextReset, type Task } from "../../lib/ipc";

interface Props {
  task: Task;
  /** 절단이 끝난 뒤 — 부모가 파생 상태(게이지 등)를 무효화한다. 구분선은 백엔드가 흘려보낸다. */
  onReset: () => void;
}

/** 끊을 벤더 세션이 남아 있지 않은 상태. 백엔드도 같은 셋을 거부하므로 여기서는 버튼을 숨긴다 —
 *  이어질 턴이 없는 작업에 캡슐을 남기면 아무도 읽지 않는다. */
const FINISHED = new Set(["Done", "Discarded", "Failed"]);

/** 확인 문구. 무엇이 사라지고 무엇이 남는지를 그 자리에서 다 말한다 —
 *  되돌릴 수 없는 동작이고, "컨텍스트를 비웁니다"만으로는 코드까지 날아가는지 알 수 없다. */
const CONFIRM =
  "이 세션의 대화 컨텍스트를 비웁니다.\n\n" +
  "· 지금까지의 작업 요약(캡슐)이 먼저 저장됩니다 — 변경 파일 목록과 작업 캔버스를 포함합니다.\n" +
  "· 다음 턴부터 에이전트는 새 대화로 시작하며, 그 캡슐이 첫 메시지 앞에 함께 전달됩니다.\n" +
  "· 코드·worktree·컨텍스트 파일은 그대로입니다. 화면의 이전 대화도 남습니다(구분선이 그어집니다).\n\n" +
  "되돌릴 수 없습니다. 계속할까요?";

/**
 * 컨텍스트 게이지 곁의 절단 버튼 — 단계가 바뀔 때 누른다.
 *
 * 트리거는 잔량이 아니라 **단계 전환**이다. 문서화를 마치고 구현에 들어가는 시점이라면 잔량이
 * 80%여도 비우는 것이 맞다 — 앞선 탐색은 이미 산출물로 압축돼 있고, 들고 가면 토큰만 먹는다.
 * 그래서 게이지 임계에 매달린 자동 동작이 아니라 사람이 누르는 버튼이다.
 *
 * 게이지 옆에 있는 이유는 잔량이 두 번째 트리거이기 때문이다 — 차오르면 어차피 해야 한다.
 */
export function ContextResetButton({ task, onReset }: Props): ReactElement | null {
  const [busy, setBusy] = useState(false);
  if (FINISHED.has(task.state)) return null;

  const run = () => {
    if (busy || !window.confirm(CONFIRM)) return;
    setBusy(true);
    convoContextReset(task.id)
      .then(
        () => {
          onReset();
          window.alert("컨텍스트를 비웠습니다. 다음 메시지에 작업 요약(캡슐)이 함께 전달됩니다.");
        },
        // 주입이 실패하면 백엔드가 세션을 끊지 않는다. 그 계약을 사용자에게도 그대로 알린다 —
        // "실패했다"만 보고 컨텍스트가 날아갔다고 오해하면 다음 행동이 달라진다.
        (error) => window.alert(`컨텍스트를 비우지 못했습니다(세션은 그대로입니다): ${error}`),
      )
      .finally(() => setBusy(false));
  };

  return (
    <button
      type="button"
      className="shrink-0 self-end rounded px-1.5 pb-0.5 text-xs text-text-muted hover:bg-raised hover:text-text disabled:opacity-50"
      onClick={run}
      disabled={busy}
      title="컨텍스트 비우기 — 작업 요약(캡슐)만 들고 새 대화로 이어간다"
      aria-label="컨텍스트 비우기"
    >
      비우기
    </button>
  );
}
