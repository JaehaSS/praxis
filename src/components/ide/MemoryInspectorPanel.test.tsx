import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ContextReport } from "../../lib/ipc";
import { MemoryInspectorPanel } from "./MemoryInspectorPanel";

function report(overrides: Partial<ContextReport> = {}): ContextReport {
  return {
    vendors: [],
    injected: [],
    capture_enabled: false,
    memory_count: 3,
    memory_counts: {
      scope_total: 3,
      actionable: 3,
      verified: 0,
      eligible: 0,
    },
    projection: { state: "applied", selected_count: 0 },
    ...overrides,
  } as ContextReport;
}

describe("MemoryInspectorPanel zero-injection reason", () => {
  it("shows the current review queue and treats capture as secondary context", () => {
    const html = renderToStaticMarkup(
      <MemoryInspectorPanel task={{ host: "local", id: 7 }} report={report()} onClose={() => {}} />,
    );

    expect(html).toContain("현재 검토 필요 3건");
    expect(html).toContain("현재 기준");
    expect(html).toContain("자동 캡처 OFF");
    expect(html).not.toContain("자동 캡처가 꺼져 있습니다 — 설정에서 켜세요.");
  });

  it("labels a missing immutable projection receipt as a legacy task", () => {
    const html = renderToStaticMarkup(
      <MemoryInspectorPanel
        task={{ host: "local", id: 7 }}
        report={report({ projection: null } as Partial<ContextReport>)}
        onClose={() => {}}
      />,
    );

    expect(html).toContain("구버전 작업");
    expect(html).toContain("projection receipt");
  });

  it("keeps matching duplicate receipts and labels legacy versions as unconfirmed", () => {
    const injected = [
      { memory_id: 4, version: null, kind: "claim", content: "current body must not become history", confidence: null, evidence_count: 0, evidence_status: null, target_hash: null, target_paths: [], renderer_version: null, injected_at: 10, outcome: null, exists: true },
      { memory_id: 4, version: 2, kind: "claim", content: "immutable receipt body", confidence: null, evidence_count: 0, evidence_status: null, target_hash: null, target_paths: [], renderer_version: null, injected_at: 10, outcome: null, exists: true },
    ];
    const html = renderToStaticMarkup(<MemoryInspectorPanel task={{ host: "remote", id: 7 }} report={report({ injected })} selectedMemoryId={4} onClose={() => {}} />);
    expect(html.match(/claim/g)).toHaveLength(2);
    expect(html).toContain("과거 사용 기록 · 버전 미확인");
    expect(html).toContain("v2");
    expect(html).toContain("immutable receipt body");
    expect(html).not.toContain("current body must not become history");
    const absent = renderToStaticMarkup(<MemoryInspectorPanel task={{ host: "remote", id: 7 }} report={report({ injected })} selectedMemoryId={99} onClose={() => {}} />);
    expect(absent).toContain("선택 메모리의 전달 기록 확인 불가");
    expect(absent).not.toContain("immutable receipt body");
  });
});
