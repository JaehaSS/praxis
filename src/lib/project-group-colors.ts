/** 사이드바 그룹 식별에만 쓰는 고정 프리셋 — 저장값을 CSS로 직접 쓰지 않는다. */
export const PROJECT_GROUP_COLORS = [
  { id: "blue", label: "파랑", accent: "#3b82f6", tint: "#3b82f61a" },
  { id: "violet", label: "보라", accent: "#8b5cf6", tint: "#8b5cf61a" },
  { id: "rose", label: "분홍", accent: "#f43f5e", tint: "#f43f5e1a" },
  { id: "orange", label: "주황", accent: "#f97316", tint: "#f973161a" },
  { id: "green", label: "초록", accent: "#22c55e", tint: "#22c55e1a" },
  { id: "slate", label: "회색", accent: "#64748b", tint: "#64748b1a" },
] as const;

export type ProjectGroupColor = (typeof PROJECT_GROUP_COLORS)[number];
export type ProjectGroupColorId = ProjectGroupColor["id"];

export const getProjectGroupColor = (color: unknown): ProjectGroupColor | undefined =>
  PROJECT_GROUP_COLORS.find((candidate) => candidate.id === color);

export const isProjectGroupColorId = (color: unknown): color is ProjectGroupColorId =>
  getProjectGroupColor(color) !== undefined;
