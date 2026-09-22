export const AGENT_ROLES = [
  "planner",
  "researcher",
  "implementer",
  "tester",
  "reviewer",
] as const;

export type AgentRole = (typeof AGENT_ROLES)[number];

export interface AgentRoleProfile {
  key: AgentRole;
  label: string;
  agentNoun: string;
  stationLabel: string;
  glyph: string;
  spriteVariant: number;
}

export const DEFAULT_AGENT_ROLE: AgentRole = "implementer";

export const AGENT_ROLE_PROFILES: readonly AgentRoleProfile[] = [
  {
    key: "planner",
    label: "계획",
    agentNoun: "planner",
    stationLabel: "PLAN BOARD",
    glyph: "◇",
    spriteVariant: 4,
  },
  {
    key: "researcher",
    label: "탐색",
    agentNoun: "scout",
    stationLabel: "EXPLORE ARCHIVE",
    glyph: "⌕",
    spriteVariant: 0,
  },
  {
    key: "implementer",
    label: "구현",
    agentNoun: "builder",
    stationLabel: "BUILD PODS",
    glyph: "⌘",
    spriteVariant: 1,
  },
  {
    key: "tester",
    label: "테스트",
    agentNoun: "tester",
    stationLabel: "TEST BENCH",
    glyph: "△",
    spriteVariant: 3,
  },
  {
    key: "reviewer",
    label: "리뷰",
    agentNoun: "reviewer",
    stationLabel: "REVIEW CORE",
    glyph: "◆",
    spriteVariant: 2,
  },
];

export function normalizeAgentRole(role?: string | null): AgentRole {
  const match = AGENT_ROLES.find((candidate) => candidate === role?.trim());
  return match ?? DEFAULT_AGENT_ROLE;
}

export function agentRoleProfile(role?: string | null): AgentRoleProfile {
  const normalized = normalizeAgentRole(role);
  return AGENT_ROLE_PROFILES.find(({ key }) => key === normalized) ?? AGENT_ROLE_PROFILES[2];
}

/** 역할별 키워드 — 한국어는 부분 매칭, 영어는 단어 경계(\b)로 오탐(예: "latest"의 test)을 막는다. */
const ROLE_KEYWORDS: readonly { role: AgentRole; ko: readonly string[]; en: readonly string[] }[] = [
  {
    role: "reviewer",
    ko: ["리뷰", "검토"],
    en: ["review", "audit"],
  },
  {
    role: "tester",
    ko: ["테스트", "테스트 코드", "검증 코드"],
    en: ["test", "qa", "coverage"],
  },
  {
    role: "planner",
    ko: ["계획", "플랜", "설계", "기획", "아키텍처"],
    en: ["plan", "design", "architecture", "prd", "roadmap"],
  },
  {
    role: "researcher",
    ko: ["조사", "분석", "탐색", "파악", "원인"],
    en: ["research", "investigate", "explore", "analyze"],
  },
];

function matchesKeyword(lowerInstruction: string, ko: readonly string[], en: readonly string[]): boolean {
  if (ko.some((keyword) => lowerInstruction.includes(keyword))) return true;
  return en.some((keyword) => new RegExp(`\\b${keyword}\\b`).test(lowerInstruction));
}

/** 지시문에서 역할을 자동 추론한다 — 사용자가 고르지 않고 에이전트가 배정. */
export function inferAgentRole(instruction: string): AgentRole {
  const trimmed = instruction.trim();
  if (!trimmed) return DEFAULT_AGENT_ROLE;
  const lower = trimmed.toLowerCase();
  for (const { role, ko, en } of ROLE_KEYWORDS) {
    if (matchesKeyword(lower, ko, en)) return role;
  }
  return DEFAULT_AGENT_ROLE;
}

function repositoryName(repository: string): string {
  return repository.split("/").filter(Boolean).pop() ?? repository;
}

export function projectAgentName(repository: string, role: AgentRole, taskId: number): string {
  const profile = agentRoleProfile(role);
  const suffix = String(Math.abs(taskId)).padStart(2, "0");
  return `${repositoryName(repository)}-${profile.agentNoun}-${suffix}`;
}
