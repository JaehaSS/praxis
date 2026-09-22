import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MemoryLifecycleGuide } from "./MemoryLifecycleGuide";

describe("MemoryLifecycleGuide", () => {
  it("distinguishes memory from self-improvement and names the actual effect", () => {
    const html = renderToStaticMarkup(<MemoryLifecycleGuide />);

    expect(html).toContain("후보부터 승인·보관까지 관리하는 저장소");
    expect(html).toContain("작업 회고에서 메모리 후보를 제안하는 검토함");
    expect(html).toContain("제안 → 후보 → 근거 확인·승인 → 다음 작업에서 선택·적용");
    expect(html).toContain("프로젝트 컨텍스트 파일(AGENTS.md)");
    expect(html).toContain("모델 학습이나 현재 작업을 직접 바꾸지 않습니다");
    expect(html).toContain("유효한 근거가 있는 비휴면 항목");
    expect(html).toContain("항상 적용 정책 또는 관련성 선택");
    expect(html).toContain("적용 미리보기");
    expect(html).toContain("사용이력");
  });
});
