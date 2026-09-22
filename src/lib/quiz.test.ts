import { describe, expect, it } from "vitest";

import type { TaskActivityRow } from "./ipc";
import { QUIZ_THRESHOLD_SECS, gateIsOpen, modeFor, shouldOffer, toSeconds } from "./quiz";

/** 백엔드 `quiz::gate::tests`와 같은 경계값을 본다 — 둘이 어긋나면 게이트가 딴 데서 열린다. */
describe("shouldOffer", () => {
  it("임계값을 넘겨야 연다", () => {
    expect(shouldOffer(1, 0, 25)).toBe(false);
    expect(shouldOffer(24, 0, 25)).toBe(false);
    expect(shouldOffer(25, 0, 25)).toBe(true);
    expect(shouldOffer(60, 0, 25)).toBe(true);
  });

  it("시계가 뒤로 가도 열리지 않는다", () => {
    expect(shouldOffer(0, 10, 25)).toBe(false);
    expect(shouldOffer(-5, 0, 25)).toBe(false);
  });

  it("임계값 0이면 항상 연다", () => {
    expect(shouldOffer(0, 0, 0)).toBe(true);
  });

  it("기본 임계값은 25초 — 밀리초로 두면 7시간이 된다", () => {
    expect(QUIZ_THRESHOLD_SECS).toBe(25);
    expect(shouldOffer(30, 0)).toBe(true);
    expect(shouldOffer(20, 0)).toBe(false);
  });
});

describe("gateIsOpen", () => {
  const row = (taskId: number, startedAt: number): TaskActivityRow => ({
    task_id: taskId,
    last_operation: null,
    last_event_at: startedAt,
    started_at: startedAt,
  });

  it("작업이 없으면 열지 않는다", () => {
    expect(gateIsOpen([], 1_000)).toBe(false);
  });

  it("하나라도 오래 기다렸으면 연다", () => {
    expect(gateIsOpen([row(1, 990), row(2, 900)], 1_000)).toBe(true);
  });

  it("전부 짧으면 열지 않는다", () => {
    expect(gateIsOpen([row(1, 990), row(2, 985)], 1_000)).toBe(false);
  });
});

describe("toSeconds", () => {
  it("밀리초를 초로 내린다", () => {
    expect(toSeconds(1_000)).toBe(1);
    expect(toSeconds(1_999)).toBe(1);
  });
});

/** 무엇을 띄울지 — 띄우지 않을지 (이슈 #87). */
describe("modeFor", () => {
  it("낼 문제가 있으면 퀴즈다", () => {
    expect(modeFor({ askable: 1, pending_review: 0 })).toBe("quiz");
    // 검수가 밀려 있어도 낼 수 있으면 푸는 쪽이 먼저다.
    expect(modeFor({ askable: 3, pending_review: 5 })).toBe("quiz");
  });

  it("낼 것이 없고 검수만 남았으면 검수다", () => {
    // 도메인 문제는 승인 전엔 출제되지 않으므로 이 조합이 흔하다. 여기서 "문제가 없습니다"를
    // 띄우면 이미 만들어진 문제를 두고 등록을 안내하는 셈이 된다.
    expect(modeFor({ askable: 0, pending_review: 2 })).toBe("review");
  });

  it("둘 다 없으면 아무것도 띄우지 않는다", () => {
    expect(modeFor({ askable: 0, pending_review: 0 })).toBeNull();
  });
});

describe("modeFor — 인사이트", () => {
  const empty = { askable: 0, pending_review: 0 };

  it("퀴즈가 있으면 인사이트보다 퀴즈를 먼저 띄운다", () => {
    // 능동 회상이 수동 읽기보다 학습 이득이 크다.
    expect(modeFor({ askable: 3, pending_review: 0 }, 10)).toBe("quiz");
  });

  it("복습 대기가 있으면 인사이트보다 복습이 먼저다", () => {
    expect(modeFor({ askable: 0, pending_review: 2 }, 10)).toBe("review");
  });

  it("퀴즈도 복습도 없고 인사이트만 있으면 인사이트를 띄운다", () => {
    expect(modeFor(empty, 4)).toBe("insight");
  });

  it("셋 다 없으면 열지 않는다", () => {
    // 띄울 것이 없으면 화면을 아예 열지 않는다(ADR 0114).
    expect(modeFor(empty, 0)).toBeNull();
  });

  it("인사이트 인자를 생략하면 종전과 같이 동작한다", () => {
    expect(modeFor(empty)).toBeNull();
    expect(modeFor({ askable: 1, pending_review: 0 })).toBe("quiz");
  });
});
