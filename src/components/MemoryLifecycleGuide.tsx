import type { ReactElement } from "react";

export function MemoryLifecycleGuide(): ReactElement {
  return (
    <section
      aria-label="메모리와 자기개선 차이"
      className="mt-2 rounded-lg border border-border bg-surface p-3"
    >
      <div className="grid gap-2 text-xs leading-relaxed text-text-secondary sm:grid-cols-2">
        <p>
          <strong className="font-medium text-text">메모리</strong> — 다음 작업용 지식을 후보부터
          승인·보관까지 관리하는 저장소입니다. 근거와 함께 직접 승인된 항목만 쓰일 수 있습니다.
        </p>
        <p>
          <strong className="font-medium text-text">자기개선</strong> — 작업 회고에서 메모리 후보를
          제안하는 검토함입니다. 모델 학습이나 현재 작업을 직접 바꾸지 않습니다.
        </p>
      </div>
      <p className="mt-2 text-xs font-medium text-primary-bright">
        제안 → 후보 → 근거 확인·승인 → 다음 작업에서 선택·적용
      </p>
      <p className="mt-1 text-xs leading-relaxed text-text-muted">
        승인되고 유효한 근거가 있는 비휴면 항목만, 새 작업 생성 시 항상 적용 정책 또는 관련성 선택을
        거쳐 프로젝트 컨텍스트 파일(AGENTS.md)에 반영됩니다. 결과는 적용
        미리보기와 각 항목의 사용이력에서 확인할 수 있습니다.
      </p>
    </section>
  );
}
