import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Memory } from "../lib/ipc";
import {
  EvidenceLoadStateView,
  MemoryEvidencePanel,
} from "./MemoryEvidencePanel";

const memory: Memory = {
  id: 7,
  tier: "project",
  scope_key: "/repo",
  kind: "claim",
  content: "verify retry behavior",
  source_session: null,
  confidence: 0,
  usage_count: 0,
  last_used: null,
  created_at: 1,
  knowledge_type: "claim",
  status: "candidate",
  current_version: 1,
  utility_score: 0,
  review_due_at: null,
  verified_at: null,
  stale_at: null,
  archived_at: null,
  dormant: false,
};

describe("MemoryEvidencePanel load state", () => {
  it("renders loading instead of a false empty state before the first response", () => {
    const html = renderToStaticMarkup(
      <MemoryEvidencePanel memory={memory} onChanged={async () => undefined} />,
    );

    expect(html).toContain("근거를 불러오는 중");
    expect(html).not.toContain("근거가 없습니다");
  });

  it("renders an explicit retry action without claiming the list is empty", () => {
    const html = renderToStaticMarkup(
      <EvidenceLoadStateView
        state={{ memoryId: 7, status: "error", message: "runner unavailable" }}
        onRetry={() => undefined}
      />,
    );

    expect(html).toContain("근거를 불러오지 못했습니다");
    expect(html).toContain("runner unavailable");
    expect(html).toContain("다시 시도");
    expect(html).not.toContain("근거가 없습니다");
  });
});
