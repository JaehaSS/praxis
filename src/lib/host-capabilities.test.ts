import { describe, expect, it } from "vitest";
import { hostCapabilities } from "./host-capabilities";
import { LOCAL_HOST } from "./transport";

describe("hostCapabilities", () => {
  it("로컬은 전부 할 수 있다", () => {
    expect(Object.values(hostCapabilities(LOCAL_HOST)).every(Boolean)).toBe(true);
  });

  it("원격은 ADR 0133 결정 6의 표대로 대부분 막힌다 — 세션홈 이어받기만 예외(설계 2026-09-17 결정 2)", () => {
    expect(hostCapabilities("mini1")).toEqual({
      fileOperations: false,
      interview: false,
      lsp: false,
      designPreview: false,
      baseBranch: false,
      workspaceShell: false,
      sessionAgentSwitch: false,
      convoResume: false,
      sessionHomeResume: true,
    });
  });

  it("호스트 이름이 무엇이든 local이 아니면 원격으로 본다", () => {
    // endpoint에서 파생한 익명 호스트도 원격이다 — 이름 규칙에 기대지 않는다.
    expect(hostCapabilities("runner:http://127.0.0.1:47831").lsp).toBe(false);
  });
});
