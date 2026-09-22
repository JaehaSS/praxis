import { describe, expect, it } from "vitest";
import {
  agentRoleProfile,
  DEFAULT_AGENT_ROLE,
  inferAgentRole,
  normalizeAgentRole,
  projectAgentName,
} from "./agent-role";

describe("agent roles", () => {
  it("normalizes missing and unknown legacy values to implementer", () => {
    expect(normalizeAgentRole(null)).toBe(DEFAULT_AGENT_ROLE);
    expect(normalizeAgentRole("")).toBe(DEFAULT_AGENT_ROLE);
    expect(normalizeAgentRole("manager")).toBe(DEFAULT_AGENT_ROLE);
    expect(normalizeAgentRole("  reviewer  ")).toBe("reviewer");
  });

  it("maps roles to stable stations and sprite variants", () => {
    expect(agentRoleProfile("planner")).toMatchObject({
      stationLabel: "PLAN BOARD",
      spriteVariant: 4,
    });
    expect(agentRoleProfile("tester")).toMatchObject({
      stationLabel: "TEST BENCH",
      spriteVariant: 3,
    });
  });

  it("builds a project-agent name from repository, role, and task identity", () => {
    expect(projectAgentName("/workspace/praxis", "implementer", 7)).toBe(
      "praxis-builder-07",
    );
    expect(projectAgentName("/workspace/api", "reviewer", 42)).toBe(
      "api-reviewer-42",
    );
  });

  describe("inferAgentRole", () => {
    it("infers reviewer from Korean and English review keywords", () => {
      expect(inferAgentRole("이 PR 리뷰해줘")).toBe("reviewer");
      expect(inferAgentRole("please review this PR")).toBe("reviewer");
    });

    it("infers tester from Korean and English test keywords", () => {
      expect(inferAgentRole("테스트 코드 추가해줘")).toBe("tester");
      expect(inferAgentRole("add test coverage for this module")).toBe("tester");
    });

    it("infers planner from Korean and English planning keywords", () => {
      expect(inferAgentRole("이번 스프린트 계획을 세워줘")).toBe("planner");
      expect(inferAgentRole("draft a roadmap for this feature")).toBe("planner");
    });

    it("infers researcher from Korean and English research keywords", () => {
      expect(inferAgentRole("버그 원인을 조사해줘")).toBe("researcher");
      expect(inferAgentRole("investigate the root cause")).toBe("researcher");
    });

    it("falls back to the default role for empty or unmatched instructions", () => {
      expect(inferAgentRole("")).toBe(DEFAULT_AGENT_ROLE);
      expect(inferAgentRole("   ")).toBe(DEFAULT_AGENT_ROLE);
      expect(inferAgentRole("커피 한 잔 마시자")).toBe(DEFAULT_AGENT_ROLE);
    });

    it("does not false-positive on English word boundaries", () => {
      expect(inferAgentRole("latest features 추가해줘")).toBe("implementer");
    });

    it("resolves keyword conflicts by role priority (reviewer beats tester)", () => {
      expect(inferAgentRole("테스트 코드 리뷰해줘")).toBe("reviewer");
    });
  });
});
