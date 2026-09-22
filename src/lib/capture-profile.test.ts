import { describe, it, expect } from "vitest";
import { AGENT_MODEL_CATALOG } from "./models";

/**
 * 캡처 프로파일 UI는 `AGENT_MODEL_CATALOG.claude`로 "확인되지 않은 모델" 경고를 띄운다.
 * 백엔드 기본값(`capture::invoke::DEFAULT_MODEL` = "sonnet", `DEFAULT_EFFORT` = "low")이
 * 이 카탈로그를 벗어나면, **설정 화면이 자기 기본값을 오타처럼 경고한다.**
 *
 * 두 값은 Rust 쪽 `defaults_are_accepted_by_existing_validators`가 각각 고정하고 있고,
 * 여기서는 그 값들이 프런트 카탈로그와도 만나는지를 본다 — 어긋남은 조용하다.
 */
describe("캡처 프로파일 기본값과 모델 카탈로그", () => {
  const claude = AGENT_MODEL_CATALOG.claude ?? [];

  it("백엔드 기본 모델 sonnet이 claude 카탈로그에 있다", () => {
    expect(claude.map((m) => m.id)).toContain("sonnet");
  });

  it("백엔드 기본 effort low가 후보 목록에 있다", () => {
    const efforts = claude.find((m) => m.reasoningEfforts?.length)
      ?.reasoningEfforts;
    expect(efforts).toBeDefined();
    expect(efforts).toContain("low");
  });

  it("effort 후보에 claude가 거부하는 ultra가 없다", () => {
    // claude CLI는 low~max만 받는다. 목록에 ultra가 섞이면 저장 시 백엔드가 거부한다.
    const efforts = claude.find((m) => m.reasoningEfforts?.length)
      ?.reasoningEfforts;
    expect(efforts).not.toContain("ultra");
  });
});
