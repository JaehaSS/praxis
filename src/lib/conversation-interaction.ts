import { invoke } from "@tauri-apps/api/core";

// 질문 세션을 열 수 있는 로컬 에이전트. 백엔드 `interaction::runtime_for_agent`와 짝이다.
export const QUESTION_AGENTS = ["codex", "claude"];

export interface QuestionAnswer { question_id: string; option_id: string | null; text: string | null }
export interface Question { id: string; question: string; options: { id: string; label: string; description: string }[]; allow_free_text: boolean; is_secret: false }
export interface AnswerReceipt { request_id: string; state: string }
export interface Interaction { id: string; execution_id: string; call_id: string; questions: { kind: "clarification"; questions: Question[] }; state: string; reason: string | null; revision: number; expires_at: number; draft: QuestionAnswer[]; draft_revision: number; receipt: AnswerReceipt | null }
export interface InteractionSnapshot { enabled: boolean; execution_id: string | null; phase: string; items: Interaction[] }
export const interactionSnapshot = (id: number) => invoke<InteractionSnapshot>("interaction_snapshot", { id });
export const saveQuestionDraft = (id: number, item: Interaction, answers: QuestionAnswer[], revision: number) => invoke<number>("interaction_draft", { id, executionId: item.execution_id, interactionId: item.id, answers, revision });
export const answerQuestion = (id: number, item: Interaction, requestId: string, answers: QuestionAnswer[]) => invoke<AnswerReceipt>("interaction_answer", { id, executionId: item.execution_id, interactionId: item.id, requestId, answers });
export const questionReceipt = (id: number, requestId: string) => invoke<AnswerReceipt>("interaction_receipt", { id, requestId });
export const retryQuestionCleanup = (id: number) => invoke<void>("interaction_cleanup_retry", { id });
export function answersComplete(item: Interaction, answers: QuestionAnswer[]): boolean {
  return item.questions.questions.every((q) => {
    const a = answers.find((v) => v.question_id === q.id);
    return !!a && (a.option_id != null ? q.options.some((o) => o.id === a.option_id) : q.allow_free_text && !!a.text?.trim());
  });
}
export const receiptLabel = (state: string) => ({ claimed: "답변 접수됨", dispatching: "답변 전송 확인 중", written: "답변 전송됨 · 반영 확인 전", acknowledged: "도구 응답 반영됨", unknown: "전달 여부 불명확 · 자동 재전송하지 않음", not_found: "접수 기록을 아직 찾지 못했습니다" })[state] ?? state;
