export interface ConversationScrollPosition {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}

const FOLLOW_THRESHOLD_PX = 48;

/** 사용자가 대화 하단을 보고 있을 때만 스트리밍 출력을 계속 따라간다. */
export function isConversationNearBottom(
  position: ConversationScrollPosition,
  threshold: number = FOLLOW_THRESHOLD_PX,
): boolean {
  const remaining = position.scrollHeight - position.scrollTop - position.clientHeight;
  return remaining <= threshold;
}
