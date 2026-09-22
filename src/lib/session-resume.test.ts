import { describe, expect, it } from "vitest";
import {
  RECENTLY_ACTIVE_THRESHOLD_SECONDS,
  describeSessionResumeError,
  isDifferentRepository,
  isRecentlyActive,
} from "./session-resume";
import { SessionResumeError } from "./transport";
import type { SessionHomeSession } from "./transport";

function session(overrides: Partial<SessionHomeSession> = {}): SessionHomeSession {
  return {
    session_id: "abcd1234efgh",
    cwd: "/repo",
    last_cwd: null,
    git_branch: null,
    title: null,
    first_message: null,
    last_active: 0,
    messages: 3,
    vendor_version: null,
    host: "local",
    ...overrides,
  };
}

describe("isRecentlyActive", () => {
  it("임계값 안이면 최근 활동으로 본다", () => {
    const now = 1_000_000;
    expect(isRecentlyActive(now - 1, now)).toBe(true);
    expect(isRecentlyActive(now - (RECENTLY_ACTIVE_THRESHOLD_SECONDS - 1), now)).toBe(true);
  });

  it("임계값 밖이면 최근 활동이 아니다", () => {
    const now = 1_000_000;
    expect(isRecentlyActive(now - RECENTLY_ACTIVE_THRESHOLD_SECONDS, now)).toBe(false);
    expect(isRecentlyActive(now - 60 * 60, now)).toBe(false);
  });
});

describe("isDifferentRepository", () => {
  it("cwd가 저장소 안이면 false", () => {
    expect(isDifferentRepository(session({ cwd: "/repo" }), "/repo")).toBe(false);
    expect(isDifferentRepository(session({ cwd: "/repo/sub" }), "/repo")).toBe(false);
  });

  it("cwd가 저장소 밖이면 true", () => {
    expect(isDifferentRepository(session({ cwd: "/other" }), "/repo")).toBe(true);
  });

  it("cwd가 없으면 last_cwd로 대신 본다", () => {
    expect(isDifferentRepository(session({ cwd: null, last_cwd: "/repo/sub" }), "/repo")).toBe(false);
    expect(isDifferentRepository(session({ cwd: null, last_cwd: "/other" }), "/repo")).toBe(true);
  });

  it("cwd·last_cwd가 둘 다 없으면 경고 쪽으로 기운다(true)", () => {
    expect(isDifferentRepository(session({ cwd: null, last_cwd: null }), "/repo")).toBe(true);
  });

  it("접두 문자열만 같고 실제로는 다른 저장소면 true (예: /repo vs /repo-2)", () => {
    expect(isDifferentRepository(session({ cwd: "/repo-2" }), "/repo")).toBe(true);
  });
});

describe("describeSessionResumeError", () => {
  it("not_found는 존재·인가를 구분하지 않는다", () => {
    const error = new SessionResumeError("이어받기에 실패해 작업을 시작하지 않았습니다: x", "not_found");
    expect(describeSessionResumeError(error)).toEqual({
      message: "찾을 수 없거나 접근 권한이 없습니다",
    });
  });

  it("conflict는 작업 id를 담아 특정한다", () => {
    const error = new SessionResumeError("이미 #12 작업이 이어가고 있습니다", "conflict", 12);
    expect(describeSessionResumeError(error)).toEqual({
      message: "이미 #12 작업이 이어가고 있습니다",
      taskId: 12,
    });
  });

  it("conflict인데 작업 id를 모르면 일반 문구로 대체한다", () => {
    const error = new SessionResumeError("충돌", "conflict");
    expect(describeSessionResumeError(error)).toEqual({
      message: "이미 다른 작업이 이어가고 있습니다",
      taskId: undefined,
    });
  });
});
