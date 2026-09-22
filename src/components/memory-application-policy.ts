import type { ApplicationPolicy, Memory } from "../lib/ipc";

/**
 * 항상-적용 지정 가능 여부. Rust `application_policy::ensure_eligible`의 UI 거울이다 —
 * 여기서 통과시켜도 서버가 다시 판정하므로, 이 함수의 역할은 **왜 안 되는지 먼저 알려주는 것**이지
 * 보안 게이트가 아니다.
 */
export type Designatability =
  | { kind: "designatable" }
  /** 이미 지정됨 — 해제만 제공한다. */
  | { kind: "designated" }
  | { kind: "blocked"; reason: string };

/**
 * 구버전 Runner 응답에는 `application_policy` 필드가 없다. 그 경우 `null`을 돌려주고
 * 호출측은 제어를 **숨긴다** — 에러로 표시하면 원격이 구버전인 것뿐인데 고장으로 보인다.
 */
export function policyOf(memory: Memory): ApplicationPolicy | null {
  return memory.application_policy ?? null;
}

export function designatability(memory: Memory): Designatability {
  const policy = policyOf(memory);
  // 지정 해제는 어떤 상태에서도 열어 둔다 — stale 규칙이 작업 시작을 막을 때
  // 빠져나갈 길이 없으면 fail-closed가 덫이 된다.
  if (policy === "must_apply") return { kind: "designated" };
  if (memory.tier !== "project") {
    return { kind: "blocked", reason: "프로젝트 범위 메모리만 항상 적용으로 지정할 수 있습니다" };
  }
  if (memory.knowledge_type !== "decision" && memory.knowledge_type !== "convention") {
    return { kind: "blocked", reason: "결정·규약 유형만 항상 적용으로 지정할 수 있습니다" };
  }
  if (memory.status !== "verified") {
    return { kind: "blocked", reason: "검증된 메모리만 항상 적용으로 지정할 수 있습니다" };
  }
  return { kind: "designatable" };
}

/** 지정/해제 전에 사람에게 보여줄 문구. 무엇이 달라지는지를 말한다. */
export function confirmMessage(memory: Memory): string {
  return policyOf(memory) === "must_apply"
    ? "항상 적용을 해제할까요? 이후에는 검색 관련도에 따라 선택됩니다."
    : "이 규칙을 항상 적용할까요? 검색 순위와 무관하게 모든 새 작업의 컨텍스트에 들어갑니다.";
}

export interface PreviewGroups {
  mustApply: Memory[];
  relevant: Memory[];
}

/**
 * 미리보기를 두 그룹으로 나눈다. 서버는 이미 지정 규칙을 앞에 고정해 보내므로
 * 여기서는 순서를 바꾸지 않고 분류만 한다 — 재정렬하면 실제 투영 순서와 표시가 갈린다.
 */
export function groupPreview(hits: Memory[]): PreviewGroups {
  return {
    mustApply: hits.filter((hit) => hit.application_policy === "must_apply"),
    relevant: hits.filter((hit) => hit.application_policy !== "must_apply"),
  };
}
