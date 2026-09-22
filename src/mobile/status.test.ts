import { describe, expect, it } from "vitest";
import type { Task } from "../lib/ipc";
import type { RunnerHealth } from "../lib/transport/runner";
import {
  actionableCount,
  classifyStatus,
  describeConnection,
  formatAge,
  QUIET_AFTER_SECS,
  sortForMobile,
  taskStateLabel,
} from "./status";

function task(id: number, state: string, updated_at = 0): Task {
  return {
    id,
    host: "local",
    repo: "/repo",
    branch: "b",
    base: "main",
    worktree_path: "/wt",
    instruction: `작업 ${id}`,
    state,
    created_at: 0,
    updated_at,
    mode: "terminal",
  };
}

const health = (last_event_at: number | null): RunnerHealth => ({
  status: "ok",
  bind: "127.0.0.1:47831",
  max_concurrent_tasks: 2,
  recovered_tasks: 0,
  retention_days: 60,
  execution_policy: "always_approve",
  last_event_at,
});

describe("taskStateLabel", () => {
  it("주요 상태를 한국어 라벨과 색으로 매핑한다", () => {
    expect(taskStateLabel("AwaitingReview")).toEqual({ label: "검토 대기", tone: "awaiting" });
    expect(taskStateLabel("Running")).toEqual({ label: "진행 중", tone: "running" });
    expect(taskStateLabel("Done")).toEqual({ label: "완료", tone: "done" });
    expect(taskStateLabel("Failed")).toEqual({ label: "실패", tone: "failed" });
  });

  it("모르는 상태는 지어내지 않고 원문을 보여준다", () => {
    expect(taskStateLabel("SomethingNew")).toEqual({ label: "SomethingNew", tone: "muted" });
  });

  it("답변 대기를 검토 대기와 구분한다", () => {
    expect(taskStateLabel("AwaitingReview", "question")).toEqual({
      label: "답변 대기",
      tone: "question",
    });
    // 검토 대기가 아닌 상태의 잔여 주석은 무시한다.
    expect(taskStateLabel("Running", "question")).toEqual({ label: "진행 중", tone: "running" });
  });
});

describe("sortForMobile", () => {
  it("내 행동이 필요한 것을 위로 올린다", () => {
    const sorted = sortForMobile([
      task(1, "Done"),
      task(2, "Running"),
      task(3, "AwaitingReview"),
      task(4, "Queued"),
      task(5, "PendingApproval"),
    ]);
    expect(sorted.map((t) => t.id)).toEqual([3, 5, 2, 4, 1]);
  });

  it("같은 순위면 최근 갱신이 먼저다", () => {
    const sorted = sortForMobile([task(1, "Running", 100), task(2, "Running", 300)]);
    expect(sorted.map((t) => t.id)).toEqual([2, 1]);
  });

  it("원본 배열을 변형하지 않는다", () => {
    const input = [task(1, "Done"), task(2, "AwaitingReview")];
    sortForMobile(input);
    expect(input.map((t) => t.id)).toEqual([1, 2]);
  });
});

describe("actionableCount", () => {
  it("검토·승인 대기만 센다", () => {
    const tasks = [
      task(1, "AwaitingReview"),
      task(2, "PendingApproval"),
      task(3, "Running"),
      task(4, "Done"),
    ];
    expect(actionableCount(tasks)).toBe(2);
    expect(actionableCount([])).toBe(0);
  });
});

describe("classifyStatus", () => {
  it("Runner 다운과 인증 실패를 구분한다", () => {
    // 이 구분이 없으면 폰에서 "뭘 고쳐야 하는지" 알 수 없다.
    expect(classifyStatus(502)).toBe("runner-down");
    expect(classifyStatus(503)).toBe("runner-down");
    expect(classifyStatus(504)).toBe("runner-down");
    expect(classifyStatus(401)).toBe("unauthorized");
    expect(classifyStatus(500)).toBe("error");
    expect(classifyStatus(404)).toBe("error");
  });
});

describe("describeConnection", () => {
  it("원인별로 다른 안내를 준다", () => {
    expect(describeConnection({ kind: "offline" }, 0).detail).toContain("tailnet");
    expect(describeConnection({ kind: "runner-down", status: 502 }, 0).detail).toContain("Runner");
    expect(describeConnection({ kind: "unauthorized" }, 0).detail).toContain("QR");
  });

  it("복구 가능한 상태만 재시도를 노출한다", () => {
    expect(describeConnection({ kind: "offline" }, 0).retryable).toBe(true);
    expect(describeConnection({ kind: "ok", health: health(0) }, 0).retryable).toBe(false);
  });

  it("마지막 신호가 오래되면 조용함으로 표시한다", () => {
    const now = 1_000_000;
    const fresh = describeConnection({ kind: "ok", health: health(now - 60) }, now);
    expect(fresh.tone).toBe("done");
    expect(fresh.detail).toBe("마지막 신호 1분 전");

    const quiet = describeConnection({ kind: "ok", health: health(now - QUIET_AFTER_SECS) }, now);
    expect(quiet.tone).toBe("muted");
  });

  it("이벤트가 없으면 그 사실을 그대로 말한다", () => {
    // 여기서 "정상"이라고만 하면 아무 일도 안 하는 Runner를 정상으로 오해한다.
    const view = describeConnection({ kind: "ok", health: health(null) }, 0);
    expect(view.detail).toContain("활동이 없습니다");
  });

  it("미래 시각이 와도 음수를 표시하지 않는다", () => {
    const view = describeConnection({ kind: "ok", health: health(500) }, 100);
    expect(view.detail).toBe("마지막 신호 0초 전");
  });
});

describe("formatAge", () => {
  it("단위를 넘길 때마다 표기가 바뀐다", () => {
    expect(formatAge(0)).toBe("0초");
    expect(formatAge(59)).toBe("59초");
    expect(formatAge(60)).toBe("1분");
    expect(formatAge(3599)).toBe("59분");
    expect(formatAge(3600)).toBe("1시간");
    expect(formatAge(86_399)).toBe("23시간");
    expect(formatAge(86_400)).toBe("1일");
  });
});
