import { listen as tauriListen } from "@tauri-apps/api/event";
import { b64ToBytes } from "./bytes";

/** 세션 키. 작업 PTY·워크스페이스 셸은 task id(number), 에이전트 CLI 액션은
 *  "<kind>:<vendor>"(string)를 쓴다. 필터링은 동등 비교뿐이라 둘 다 그대로 통한다. */
export type PtySessionKey = number | string;

/** id별 이벤트명 — 선택되지 않은 세션의 출력이 JS 리스너까지 오지 않도록 채널을 가른다.
 *  `prefix`는 `"pty"`·`"shell"`. Tauri 이벤트명 허용 문자는 영숫자와 `-/:_`다. */
export function ptyEventNames(
  prefix: string,
  id: PtySessionKey,
): { outputEvent: string; exitEvent: string } {
  return { outputEvent: `${prefix}://output/${id}`, exitEvent: `${prefix}://exit/${id}` };
}

interface PtyOutputPayload {
  id: PtySessionKey;
  data: string;
}

interface PtyExitPayload {
  id: PtySessionKey;
  code: number;
}

export interface AttachPtyStreamOptions {
  /** 필터링 키 — 작업 PTY·워크스페이스 셸은 taskId, 액션 셸은 "<kind>:<vendor>". */
  id: PtySessionKey;
  outputEvent: string;
  exitEvent: string;
  /** 스크롤백 replay(base64) 조회. 세션이 없거나 실패하면 빈 문자열로 취급한다. */
  fetchReplay: () => Promise<string>;
  onData: (bytes: Uint8Array) => void;
  onExit: (code: number) => void;
  /** 테스트 주입용 — 기본은 실제 Tauri `listen`. */
  listenFn?: typeof tauriListen;
}

/** replay를 선전송한 뒤 라이브 스트림을 구독한다(순서 고정 — 유실·중복 방지).
 * 반환된 cleanup은 언마운트 시 반드시 호출해 두 리스너를 해제한다. */
export async function attachPtyStream(opts: AttachPtyStreamOptions): Promise<() => void> {
  const listenFn = opts.listenFn ?? tauriListen;

  const replay = await opts.fetchReplay().catch(() => "");
  if (replay) opts.onData(b64ToBytes(replay));

  const unOut = await listenFn<PtyOutputPayload>(opts.outputEvent, (e) => {
    if (e.payload.id === opts.id) opts.onData(b64ToBytes(e.payload.data));
  });
  const unExit = await listenFn<PtyExitPayload>(opts.exitEvent, (e) => {
    if (e.payload.id === opts.id) opts.onExit(e.payload.code);
  });

  return () => {
    unOut();
    unExit();
  };
}
