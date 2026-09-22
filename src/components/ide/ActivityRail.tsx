import type { ReactElement } from "react";
import type { ActivityItem } from "../../lib/activity";
import type { ConvoStatus, Task } from "../../lib/ipc";
import { ActivityPanel } from "./ActivityPanel";
import { FLOATING_CHANNEL_WIDTH } from "./activity-rail";
import { ChannelCard } from "./ChannelCard";
import { TodaySection } from "./TodaySection";
import type { WorkContextDiff } from "./WorkContextPanel";

interface Props {
  task: Task;
  runtime: "local" | "remote";
  diff: WorkContextDiff;
  items: ActivityItem[];
  busy: boolean;
  activity: ConvoStatus | null;
  onRefreshDiff: () => void;
  onOpenSubagent: (toolId: string) => void;
  /** 최근 활동 진입점 — 긴 목록은 채널 대신 코드 열의 작업정보 탭이 맡는다. */
  onOpenRecentActivity: () => void;
  /** 할 일 채널의 Task 배지용 — App이 이미 들고 있는 목록을 그대로 받는다. */
  tasks?: Task[];
  /** 플로팅 채널 접기 — 환경 헤더의 ✕. 코드 열의 작업정보 탭에서는 전달하지 않는다(Don't #12). */
  onCollapse?: () => void;
}

/**
 * 세션 우상단에 떠 있는 채널 스택(설계 0018) — 위가 작업정보, 아래가 오늘 할 일.
 *
 * **세션 열 안에** 마운트된다. 헤더·파일 트리·코드 열은 채널의 영역이 아니고, 세션은 채널이
 * 떠 있는 동안 `FLOATING_CHANNEL_RESERVED`만큼 오른쪽을 비워 겹침을 없앤다 — absolute 배치의
 * 기준은 padding box라 이 예약은 카드 위치를 밀지 않는다.
 *
 * 컨테이너는 아래까지 내려오지만 배경도 클릭도 받지 않는다(`pointer-events-none`) — 카드만
 * 콘텐츠 높이로 떠 있고, 그 사이 공백과 아래 여백은 세션 그대로다.
 *
 * 공간이 모자라면 **작업정보가 먼저 줄어든다**(`min-h-0`, 내부 스크롤). 할 일은 줄 수가 적고
 * 눈으로 확인하는 것이 목적이라 접히면 쓸모가 없다 — 대신 화면의 45%를 넘지 않게 잘라 둔다.
 */
export function ActivityRail({ tasks, onCollapse, ...panel }: Props): ReactElement {
  return (
    <div
      className="pointer-events-none absolute top-4 right-4 bottom-4 z-30 flex flex-col gap-3"
      style={{ width: FLOATING_CHANNEL_WIDTH }}
    >
      <ChannelCard label="플로팅 작업 정보" className="min-h-0">
        <ActivityPanel {...panel} density="rail" onCollapse={onCollapse} />
      </ChannelCard>

      {/* 비면 카드째 사라진다 — 세션 화면에 빈 껍데기를 매일 띄우지 않는다(설계 0021 §12). */}
      <TodaySection density="channel" tasks={tasks} repo={panel.task.repo} />
    </div>
  );
}

/**
 * 코드 열의 `작업정보` 탭 — 파일·Diff를 부른 동안 채널이 흡수되는 자리다(ADR 0066 결정 3).
 *
 * 같은 카드 두 장을 쓴다. 옮겨 온 것이지 다른 화면이 아니라는 것을 형태가 말해야 한다.
 * 다만 떠 있지 않으므로 자리를 예약받아 흐르고, 열 하나를 온전히 쓰는 폭이라 밀도는
 * `panel`이다 — 채널에서 진입점만 두었던 최근 활동이 여기서는 목록째 펼쳐진다.
 */
export function ActivityColumnTab({ tasks, onCollapse, ...panel }: Props): ReactElement {
  // 코드 열의 작업정보 탭은 닫히지 않는다(Don't #12) — 접기 손잡이는 받아도 버린다.
  void onCollapse;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
      <ChannelCard label="작업 정보" className="shrink-0">
        <ActivityPanel {...panel} />
      </ChannelCard>

      <TodaySection density="channel" tasks={tasks} repo={panel.task.repo} />
    </div>
  );
}
