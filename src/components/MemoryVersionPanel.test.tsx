import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Memory, MemoryVersion } from "../lib/ipc";

vi.mock("@monaco-editor/react", () => ({
  DiffEditor: ({ original, modified }: { original: string; modified: string }) => (
    <div data-original={original} data-modified={modified}>diff</div>
  ),
}));

import { MemoryVersionStateView } from "./MemoryVersionPanel";

const memory: Memory = {
  id: 7,
  tier: "project",
  scope_key: "/repo",
  kind: "decision",
  content: "current content",
  source_session: null,
  confidence: 0,
  usage_count: 0,
  last_used: null,
  created_at: 1,
  knowledge_type: "decision",
  status: "candidate",
  current_version: 2,
  utility_score: 0,
  review_due_at: null,
  verified_at: null,
  stale_at: null,
  archived_at: null,
  dormant: false,
};

const versions: MemoryVersion[] = [
  {
    memory_id: 7,
    version: 2,
    content: "current content",
    knowledge_type: "decision",
    scope_snapshot: "/repo",
    created_at: 2,
    editor_kind: "human_edit",
    evidence_count: 0,
  },
  {
    memory_id: 7,
    version: 1,
    content: "previous content",
    knowledge_type: "claim",
    scope_snapshot: "/repo",
    created_at: 1,
    editor_kind: "candidate_intake",
    evidence_count: 1,
  },
];

describe("MemoryVersionPanel states", () => {
  it("separates loading, unsupported, and retryable error states", () => {
    const loading = renderToStaticMarkup(
      <MemoryVersionStateView
        memory={memory}
        state={{ memoryId: 7, status: "loading" }}
        onRetry={() => undefined}
        onRestore={async () => undefined}
      />,
    );
    const unsupported = renderToStaticMarkup(
      <MemoryVersionStateView
        memory={memory}
        state={{ memoryId: 7, status: "unsupported" }}
        onRetry={() => undefined}
        onRestore={async () => undefined}
      />,
    );
    const error = renderToStaticMarkup(
      <MemoryVersionStateView
        memory={memory}
        state={{ memoryId: 7, status: "error", message: "offline" }}
        onRetry={() => undefined}
        onRestore={async () => undefined}
      />,
    );

    expect(loading).toContain("버전 이력을 불러오는 중");
    expect(unsupported).toContain("버전 이력 미지원");
    expect(error).toContain("offline");
    expect(error).toContain("다시 시도");
  });

  it("shows a historical-to-current diff and a candidate restore action", () => {
    const html = renderToStaticMarkup(
      <MemoryVersionStateView
        memory={memory}
        state={{ memoryId: 7, status: "ready", versions }}
        onRetry={() => undefined}
        onRestore={async () => undefined}
      />,
    );

    expect(html).toContain("v1 → 현재 v2");
    expect(html).toContain("과거 근거 1건");
    expect(html).toContain("이 버전을 새 후보로 복원");
    expect(html).toContain('data-original="previous content"');
    expect(html).toContain('data-modified="current content"');
  });
});
