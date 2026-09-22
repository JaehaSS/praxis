import { describe, expect, it } from "vitest";
import type { HubUpdate, UpdateOutcome, VendorHealth } from "./ipc";
import {
  accountLine,
  authBadge,
  canUpdate,
  hubNotice,
  methodLabel,
  outcomeLine,
  updateHint,
  versionLabel,
} from "./agent-health";

const health = (over: Partial<VendorHealth> = {}): VendorHealth => ({
  vendor: "codex",
  label: "Codex",
  auth: "ok",
  auth_detail: null,
  account: null,
  plan: null,
  installed: "0.111.0",
  latest: "0.149.0",
  update_available: true,
  install_method: "npm",
  bin_path: null,
  checked_at: 0,
  ...over,
});

describe("authBadge", () => {
  it("확인 불가를 실패색으로 물들이지 않는다", () => {
    // unknown은 "못 물어봤다"이지 "로그아웃됐다"가 아니다. 빨강으로 겁주면 헛걸음을 부른다.
    expect(authBadge("unknown").color).toBe("text-text-muted");
    expect(authBadge("logged_out").color).toBe("text-status-failed");
    expect(authBadge("ok").color).toBe("text-status-done");
  });
});

describe("versionLabel", () => {
  it("업데이트가 있으면 화살표로 목적지를 보여준다", () => {
    expect(versionLabel(health())).toBe("0.111.0 → 0.149.0");
  });

  it("최신이면 최신이라고 말한다", () => {
    // 아무 말도 없으면 "확인이 안 된 건가"로 읽힌다.
    expect(
      versionLabel(health({ installed: "2.1.241", latest: "2.1.241", update_available: false })),
    ).toBe("2.1.241 · 최신");
  });

  it("registry 조회가 실패해도 설치 버전은 남긴다", () => {
    expect(versionLabel(health({ latest: null, update_available: false }))).toBe("0.111.0");
  });

  it("실행 파일을 못 찾으면 그렇게 말한다", () => {
    expect(versionLabel(health({ installed: null }))).toBe("설치 확인 불가");
  });
});

describe("accountLine", () => {
  it("있는 것만 이어 붙인다", () => {
    expect(accountLine(health({ account: "me@example.com", plan: "max" }))).toBe(
      "me@example.com · max",
    );
    expect(accountLine(health({ auth_detail: "Logged in using ChatGPT" }))).toBe(
      "Logged in using ChatGPT",
    );
    expect(accountLine(health())).toBe("");
  });
});

describe("canUpdate / updateHint", () => {
  it("설치 방식을 모르면 업데이트를 막고 이유를 남긴다", () => {
    // 회색 버튼만 있으면 왜 못 누르는지 알 수 없어 결국 터미널을 연다.
    const unknown = health({ install_method: "unknown" });
    expect(canUpdate(unknown)).toBe(false);
    expect(updateHint(unknown)).toContain("설치 방식");
  });

  it("업데이트가 있고 방식을 알면 누를 수 있고, 사유는 없다", () => {
    expect(canUpdate(health())).toBe(true);
    expect(updateHint(health())).toBeNull();
  });

  it("최신이면 방식을 몰라도 사유를 만들지 않는다", () => {
    // 업데이트할 게 없는데 "설치 방식을 모릅니다"가 뜨면 없는 문제를 만든다.
    const current = health({ update_available: false, install_method: "unknown" });
    expect(canUpdate(current)).toBe(false);
    expect(updateHint(current)).toBeNull();
  });
});

describe("methodLabel", () => {
  it("설치 방식을 사람 말로 옮긴다", () => {
    expect(methodLabel("npm")).toBe("npm 전역");
    expect(methodLabel("native")).toBe("native 설치");
    expect(methodLabel("homebrew")).toBe("Homebrew");
    expect(methodLabel("unknown")).toBe("설치 방식 미상");
  });
});

describe("outcomeLine", () => {
  const outcome = (over: Partial<UpdateOutcome> = {}): UpdateOutcome => ({
    vendor: "claude",
    label: "Claude Code",
    from: "2.1.247",
    to: "2.1.250",
    ok: true,
    error: null,
    ...over,
  });

  it("버전이 바뀌었으면 무엇에서 무엇으로 갔는지 보여준다", () => {
    expect(outcomeLine(outcome())).toBe("Claude Code 2.1.247 → 2.1.250");
  });

  it("명령이 성공해도 버전이 그대로면 변화 없음으로 읽힌다", () => {
    // `claude update`는 이미 최신일 때도 성공으로 끝난다 — 성공만 보여주면 오해를 부른다.
    expect(outcomeLine(outcome({ to: "2.1.247" }))).toBe("Claude Code · 변화 없음");
  });

  it("명령은 성공했지만 버전을 못 읽으면 모른다고 말한다", () => {
    // "변화 없음"이라 하면 모르는 것을 안다고 말하는 셈이다.
    expect(outcomeLine(outcome({ to: null }))).toBe("Claude Code · 업데이트함 · 버전 확인 실패");
  });

  it("이전 버전을 몰랐어도 변화는 변화다", () => {
    expect(outcomeLine(outcome({ from: null }))).toBe("Claude Code → 2.1.250");
  });

  it("실패는 사유까지 남긴다", () => {
    const line = outcomeLine(outcome({ ok: false, error: "PATH에서 찾을 수 없습니다: npm" }));
    expect(line).toBe("Claude Code · 실패: PATH에서 찾을 수 없습니다: npm");
  });

  it("사유를 모르는 실패도 실패로는 보인다", () => {
    expect(outcomeLine(outcome({ ok: false, error: null }))).toBe("Claude Code · 실패");
  });
});

describe("hubNotice", () => {
  const hub = (over: Partial<HubUpdate> = {}): HubUpdate => ({
    installed: "2.9.1",
    downloaded: "2.11.0",
    restart_required: true,
    ...over,
  });

  it("재시작이 필요하면 무엇이 기다리는지 알린다", () => {
    expect(hubNotice(hub())).toBe("Antigravity 2.9.1 → 2.11.0 · 앱을 재시작하면 적용됩니다");
  });

  it("최신이면 아무 말도 하지 않는다", () => {
    // 아무 일 없을 때 자리를 차지하면 잡음이 된다.
    expect(hubNotice(hub({ restart_required: false }))).toBeNull();
  });
});
