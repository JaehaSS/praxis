import { describe, expect, it, vi } from "vitest";

import {
  MAX_RETRIES_PER_ROUND,
  grillReducer,
  initialGrillState,
  isStale,
  runGrillNote,
  runGrillRound,
  type GrillAction,
  type GrillApi,
  type GrillState,
} from "./grill";
import type { GrillNote, GrillRound } from "./ipc";

const Q = { id: "q1", text: "무엇", recommendation: "이렇게", why: "때문" };

const round = (over: Partial<GrillRound> = {}): GrillRound => ({
  question: Q,
  open_threads: [],
  round: 1,
  forced_end: false,
  ...over,
});

const note = (over: Partial<GrillNote> = {}): GrillNote => ({
  slug: "s",
  markdown: "# 노트",
  revised_instruction: "새 지시문",
  unresolved: [],
  dropped: 0,
  ...over,
});

const started = (requestId = 1): GrillState =>
  grillReducer(initialGrillState(), { type: "start", instruction: "i", repo: "r", requestId });

const asking = (requestId = 1): GrillState =>
  grillReducer(started(requestId), { type: "asked", round: round(), requestId });

describe("grillReducer", () => {
  it("답변을 transcript에 누적하고 draft를 비운다", () => {
    let s = asking();
    s = grillReducer(s, { type: "draft", value: "내 답" });
    s = grillReducer(s, { type: "answer" });
    expect(s.transcript).toEqual([{ question: "무엇", recommendation: "이렇게", answer: "내 답" }]);
    expect(s.draft).toBe("");
    expect(s.current).toBeNull();
    expect(s.phase).toBe("asking");
  });

  it("추천대로는 recommendation을 답변으로 채운다", () => {
    const s = grillReducer(asking(), { type: "acceptRecommendation" });
    expect(s.transcript[0].answer).toBe("이렇게");
  });

  it("모르겠다도 유효한 답으로 기록한다", () => {
    const s = grillReducer(asking(), { type: "dontKnow" });
    expect(s.transcript[0].answer).toBe("모르겠다");
  });

  it("빈 답변은 커밋하지 않는다", () => {
    const s = grillReducer(asking(), { type: "answer" });
    expect(s.transcript).toHaveLength(0);
    expect(s.current).not.toBeNull();
  });

  it("question=null이면 noting으로 간다", () => {
    const s = grillReducer(started(), {
      type: "asked",
      round: round({ question: null, round: 4 }),
      requestId: 1,
    });
    expect(s.phase).toBe("noting");
  });

  it("상한으로 끊긴 경우 forcedEnd를 보존한다", () => {
    const s = grillReducer(started(), {
      type: "asked",
      round: round({ question: null, forced_end: true, open_threads: ["남음"] }),
      requestId: 1,
    });
    expect(s.phase).toBe("noting");
    expect(s.forcedEnd).toBe(true);
    expect(s.openThreads).toEqual(["남음"]);
  });

  it("이전 세대의 늦은 응답을 무시한다", () => {
    const s = grillReducer(started(2), { type: "asked", round: round(), requestId: 1 });
    expect(s.current).toBeNull();
    expect(s.phase).toBe("asking");
  });

  it("재시도 상한을 넘으면 error에 머문다", () => {
    const base: GrillState = {
      ...initialGrillState(),
      phase: "error",
      retries: MAX_RETRIES_PER_ROUND,
      error: "실패",
    };
    const s = grillReducer(base, { type: "retry" });
    expect(s.phase).toBe("error");
    expect(s.retries).toBe(MAX_RETRIES_PER_ROUND);
  });

  it("재시도는 카운터를 올리고 이전 단계로 돌아간다", () => {
    const failed = grillReducer(asking(), { type: "failed", error: "네트워크", requestId: 1 });
    expect(failed.phase).toBe("error");
    const retried = grillReducer(failed, { type: "retry" });
    expect(retried.retries).toBe(1);
    expect(retried.phase).toBe("answering");
    expect(retried.error).toBeNull();
  });

  it("이만 종료는 현재 질문을 버리고 노트로 간다", () => {
    const s = grillReducer(asking(), { type: "endNow" });
    expect(s.phase).toBe("noting");
    expect(s.current).toBeNull();
  });
});

describe("reset 뒤의 세대", () => {
  it("이전 세대의 늦은 asked/noted는 무시한다", () => {
    const closed = grillReducer(asking(), { type: "reset" });
    expect(grillReducer(closed, { type: "asked", round: round(), requestId: 1 })).toBe(closed);
    expect(grillReducer(closed, { type: "noted", note: note(), requestId: 1 })).toBe(closed);
  });

  it("다시 시작하면 새 세대의 asked가 반영된다", () => {
    const restarted = grillReducer(grillReducer(asking(), { type: "reset" }), {
      type: "start",
      instruction: "새 지시문",
      repo: "r",
      requestId: 2,
    });
    const s = grillReducer(restarted, { type: "asked", round: round(), requestId: 2 });
    expect(s.phase).toBe("answering");
    expect(s.current).toEqual(Q);
  });
});

describe("isStale", () => {
  it("idle에서는 stale이 아니다", () => {
    expect(isStale(initialGrillState(), "다른 지시문", "/other")).toBe(false);
  });

  it("지시문이 바뀌면 stale이다", () => {
    expect(isStale(asking(), "바뀐 지시문", "r")).toBe(true);
  });
});

const deps = (api: Partial<GrillApi>) => ({
  api: {
    round: vi.fn(),
    note: vi.fn(),
    save: vi.fn(),
    ...api,
  } as GrillApi,
  dispatch: vi.fn(),
});

describe("runGrillRound", () => {
  it("성공하면 asked를 디스패치한다", async () => {
    const d = deps({ round: vi.fn().mockResolvedValue(round()) });
    await runGrillRound(d, { repo: "r", instruction: "i", agent: "claude", requestId: 1 }, []);
    expect(d.dispatch).toHaveBeenCalledWith({ type: "asked", round: round(), requestId: 1 });
  });

  it("실패해도 예외를 던지지 않고 failed로 내린다", async () => {
    const d = deps({ round: vi.fn().mockRejectedValue(new Error("CLI 없음")) });
    await runGrillRound(d, { repo: "r", instruction: "i", agent: "claude", requestId: 1 }, []);
    expect(d.dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ type: "failed", requestId: 1 }),
    );
  });
});

describe("runGrillNote", () => {
  it("노트를 상태에 담는다", async () => {
    const d = deps({ note: vi.fn().mockResolvedValue(note()) });
    await runGrillNote(d, { repo: "r", instruction: "i", agent: "claude", requestId: 1 }, []);
    expect(d.dispatch).toHaveBeenCalledWith({ type: "noted", note: note(), requestId: 1 });
  });

  it("실패해도 예외를 던지지 않고 failed로 내린다", async () => {
    const d = deps({ note: vi.fn().mockRejectedValue(new Error("타임아웃")) });
    await runGrillNote(d, { repo: "r", instruction: "i", agent: "claude", requestId: 1 }, []);
    expect(d.dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ type: "failed", requestId: 1 }),
    );
  });
});

/** 리듀서를 물린 dispatch — 흐름 도중 reset이 최종 상태에 미치는 영향을 본다. */
const liveDeps = (api: Partial<GrillApi>) => {
  let state = initialGrillState();
  const deps = {
    api: { round: vi.fn(), note: vi.fn(), save: vi.fn(), ...api } as GrillApi,
    dispatch: (action: GrillAction) => {
      state = grillReducer(state, action);
    },
  };
  return { deps, current: () => state };
};

describe("진행 중 닫기", () => {
  const params = { repo: "r", instruction: "i", agent: "claude", requestId: 1 };

  it("runGrillRound 도중 reset하면 늦은 asked가 되살리지 못한다", async () => {
    let release = () => {};
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { deps, current } = liveDeps({
      round: vi.fn(async () => {
        await pending;
        return round();
      }),
    });
    deps.dispatch({ type: "start", instruction: "i", repo: "r", requestId: 1 });

    const flow = runGrillRound(deps, params, []);
    deps.dispatch({ type: "reset" });
    release();
    await flow;

    expect(current().phase).toBe("idle");
    expect(current().current).toBeNull();
  });

  it("runGrillNote 도중 reset하면 늦은 noted가 되살리지 못한다", async () => {
    let release = () => {};
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { deps, current } = liveDeps({
      note: vi.fn(async () => {
        await pending;
        return note();
      }),
    });
    deps.dispatch({ type: "start", instruction: "i", repo: "r", requestId: 1 });

    const flow = runGrillNote(deps, params, []);
    deps.dispatch({ type: "reset" });
    release();
    await flow;

    expect(current().phase).toBe("idle");
    expect(current().note).toBeNull();
  });
});

describe("저장 실패", () => {
  it("노트를 화면에서 지우지 않는다", () => {
    const done: GrillState = {
      ...initialGrillState(),
      phase: "done",
      note: note(),
    };
    const failed = grillReducer(done, { type: "saveFailed", error: "권한 없음" });
    expect(failed.phase).toBe("done");
    expect(failed.note).not.toBeNull();
    expect(failed.error).toBe("권한 없음");
  });

  it("저장에 성공하면 이전 실패 메시지를 지운다", () => {
    const withError: GrillState = {
      ...initialGrillState(),
      phase: "done",
      note: note(),
      error: "권한 없음",
    };
    const saved = grillReducer(withError, { type: "saved", path: "docs/explorations/x.md" });
    expect(saved.error).toBeNull();
    expect(saved.savedPath).toBe("docs/explorations/x.md");
  });
});
