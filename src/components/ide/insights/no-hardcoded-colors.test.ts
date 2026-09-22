// @ts-expect-error node builtin — 이 저장소에는 @types/node가 없다 (vite.config.ts와 같은 관례)
import { readFileSync } from "node:fs";
// @ts-expect-error node builtin — 이 저장소에는 @types/node가 없다
import { dirname, join } from "node:path";
// @ts-expect-error node builtin — 이 저장소에는 @types/node가 없다
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));

const FILES = [
  "parts.tsx",
  "AreaChart.tsx",
  "Heatmap.tsx",
  "Punchcard.tsx",
  "PlanCalendar.tsx",
  "StackedBar.tsx",
  join("..", "InsightsView.tsx"),
];

const HEX_COLOR = /#[0-9a-fA-F]{6}\b/;
const RGBA_COLOR = /rgba?\(\s*\d/;

describe("insights 컴포넌트에 하드코딩 색이 없다", () => {
  it.each(FILES)("%s", (relPath) => {
    const content = readFileSync(join(here, relPath), "utf-8");
    expect(content).not.toMatch(HEX_COLOR);
    expect(content).not.toMatch(RGBA_COLOR);
  });
});
