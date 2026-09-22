// 대화 이벤트 → 모바일 표시 아이템. 순수 로직. (설계 0013 §10 M2)
//
// 데스크톱 ConversationView의 매퍼를 그대로 쓰지 않는다. 그 파일을 import하면
// react-markdown·SubagentCard 등 데스크톱 컴포넌트 트리가 모바일 번들에 딸려 온다.
// **타입만** 가져오고(빌드 시 지워진다) 매핑은 모바일에 필요한 만큼만 새로 쓴다.
import type { ConvoEventLike } from "../components/ide/ConversationView";

export type MobileConvoItem =
  | { role: "user"; text: string }
  | { role: "text"; text: string }
  | { role: "tool"; name: string; summary: string }
  | { role: "result"; summary: string; error: boolean }
  | { role: "error"; text: string }
  | { role: "meta"; cost: number; turns: number };

/** Runner task output 한 행을 대화 이벤트로 해석 — kind 태그 JSON이 아니면 터미널 조각이다. */
export function parseConvoEvent(data: string): ConvoEventLike | null {
  try {
    const value: unknown = JSON.parse(data);
    if (
      typeof value === "object" &&
      value !== null &&
      typeof (value as { kind?: unknown }).kind === "string"
    ) {
      return value as ConvoEventLike;
    }
  } catch {
    /* 대화 이벤트가 아님 */
  }
  return null;
}

/**
 * 이벤트 하나를 표시 아이템으로. 모르는 kind는 **버린다** — 폰 화면에서 정체불명의
 * 원시 JSON을 보여주느니 없는 편이 낫고, 필요하면 터미널 탭에 원문이 남아 있다.
 */
export function eventToItem(event: ConvoEventLike): MobileConvoItem | null {
  switch (event.kind) {
    case "user":
      return { role: "user", text: event.text ?? "" };
    case "text":
    case "assistant":
      return { role: "text", text: event.text ?? "" };
    case "tool_use":
      return { role: "tool", name: event.name ?? "tool", summary: event.summary ?? "" };
    case "tool_result":
      return {
        role: "result",
        summary: event.summary ?? "",
        error: event.is_error === true,
      };
    case "error":
      return { role: "error", text: event.text ?? "" };
    case "result":
      return { role: "meta", cost: event.cost_usd ?? 0, turns: event.num_turns ?? 0 };
    default:
      return null;
  }
}

export function toItems(events: ConvoEventLike[]): MobileConvoItem[] {
  return events
    .map(eventToItem)
    .filter((item): item is MobileConvoItem => item !== null)
    // 빈 텍스트 말풍선은 화면만 차지한다.
    .filter((item) => !(item.role === "text" && item.text.trim() === ""));
}

/**
 * 낙관적으로 그린 user 말풍선과 서버가 되돌려준 durable user 이벤트가 겹치면 하나만 남긴다.
 * 이게 없으면 보낸 메시지가 두 번 보인다.
 */
export function appendItems(
  previous: MobileConvoItem[],
  incoming: MobileConvoItem[],
): MobileConvoItem[] {
  const last = previous[previous.length - 1];
  const first = incoming[0];
  if (last?.role === "user" && first?.role === "user" && last.text === first.text) {
    return [...previous, ...incoming.slice(1)];
  }
  return [...previous, ...incoming];
}

/** 대화가 아직 도는 중인지 — `result`가 오면 한 턴이 끝난 것이다. */
export function isTurnComplete(events: ConvoEventLike[]): boolean {
  return events.some((event) => event.kind === "result");
}
