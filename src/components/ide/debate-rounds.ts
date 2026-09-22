import type { DebateEndReason, DebateSpeaker } from "../../lib/ipc";
import { eventToItems, type ConvoEventLike, type ConvoItem } from "./ConversationView";

/** 토론 이벤트 — 저장 이력·라이브 스트림 공용 형태에 발화자만 얹은 것. */
export type DebateEventLike = ConvoEventLike & {
  speaker?: DebateSpeaker;
  reason?: DebateEndReason;
};

/** 라운드 하나 — 같은 행에 놓이는 좌우 발화와, 어느 면에도 속하지 않는 공통 영역. */
export interface DebateRound {
  /** 사용자 발화마다 1부터 다시 센다 — 백엔드의 라운드 상한이 그 단위로 걸린다. */
  no: number;
  left: ConvoItem[];
  right: ConvoItem[];
  /** 이 라운드를 연 사용자 발화. */
  user?: string;
  /** 발화자 미상 이벤트 — 좌우 어느 면으로도 접지 않는다(오귀속 금지). */
  common: ConvoItem[];
}

export interface DebateTranscript {
  /** 첫 라운드 이전의 이벤트 전부. 토론 이전 대화가 여기 남는다. */
  preamble: ConvoItem[];
  rounds: DebateRound[];
  /** 마지막 `debate_ended`. 화면 상태는 `running | ended(reason)` 둘뿐이다. */
  ended?: DebateEndReason;
}

/**
 * 이 이벤트로 시퀀스가 끝났는가 — 컴포저 잠금(busy)을 푸는 유일한 기준.
 *
 * 토론은 면마다 `result`를 낸다. 그것으로 풀면 좌측이 끝나는 순간 컴포저가 열려 우측이 도는
 * 중에 `convo_send`가 겹친다. 시퀀스를 닫는 것은 `debate_ended`뿐이고, 토론이 아닌 턴은
 * `speaker` 없는 `result` 하나로 끝난다.
 */
export const endsSequence = (ev: DebateEventLike): boolean =>
  ev.kind === "debate_ended" || (ev.kind === "result" && ev.speaker == null);

/**
 * 이벤트 배열 → 라운드 행 grid의 재료.
 *
 * 규칙 셋뿐이다. ① `speaker`가 없으면 공통 영역이다 — 없다는 것은 좌측이 아니라 **미상**이라
 * 한쪽 면으로 접으면 오귀속이 된다. ② 우측 다음에 좌측이 오면 새 라운드다. ③ 사용자 발화는
 * 라운드 번호를 1로 되돌린다 — 상한이 발화 한 건의 라운드 시퀀스에 걸리므로 이어 세면
 * `라운드 4/3`이 나온다.
 */
export function debateRounds(events: readonly DebateEventLike[]): DebateTranscript {
  const preamble: ConvoItem[] = [];
  const rounds: DebateRound[] = [];
  let current: DebateRound | null = null;
  let lastSpeaker: DebateSpeaker | null = null;
  let base = 0;
  let ended: DebateEndReason | undefined;

  const open = (user?: string): DebateRound => {
    const round: DebateRound = { no: rounds.length - base + 1, left: [], right: [], common: [], user };
    rounds.push(round);
    current = round;
    lastSpeaker = null;
    return round;
  };

  const all = events as ConvoEventLike[];
  events.forEach((ev, index) => {
    if (ev.kind === "debate_ended") {
      ended = ev.reason ?? "aborted";
      return;
    }
    const items = eventToItems(ev, index, all);
    if (ev.speaker == null) {
      if (ev.kind === "user") {
        // 새 발화는 새 시퀀스다 — 번호를 되돌리고 직전 배너를 지운다.
        base = rounds.length;
        ended = undefined;
        open(ev.text ?? "");
        return;
      }
      (current ? current.common : preamble).push(...items);
      return;
    }
    let round = current;
    if (round == null || (ev.speaker === "left" && lastSpeaker === "right")) round = open();
    (ev.speaker === "left" ? round.left : round.right).push(...items);
    lastSpeaker = ev.speaker;
  });

  return { preamble, rounds, ended };
}

/** 지금 도는 면 — 커서는 하나뿐이라 마지막 라운드에서 마지막으로 말한 쪽에 둔다. */
export function activeSide(transcript: DebateTranscript): DebateSpeaker {
  const last = transcript.rounds[transcript.rounds.length - 1];
  return last != null && last.right.length > 0 ? "right" : "left";
}

/** 합의 결론 — 마지막 라운드에서 마지막으로 나온 발화 본문. `결론 복사`가 넘기는 것. */
export function consensusText(transcript: DebateTranscript): string {
  const last = transcript.rounds[transcript.rounds.length - 1];
  if (!last) return "";
  const side = last.right.length > 0 ? last.right : last.left;
  const text = [...side].reverse().find((item) => item.role === "text");
  return text?.role === "text" ? text.text : "";
}
