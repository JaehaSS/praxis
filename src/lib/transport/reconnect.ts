// Runner 이벤트 구독의 재연결 정책 — 순수 로직만. (설계 0013 §7.3)
//
// 데스크톱은 앱이 상주해 이 경로가 드물게 돌지만, 모바일 PWA는 백그라운드에서 수시로
// 죽고 네트워크가 바뀐다. 고정 지연 재연결과 메모리 전용 커서로는 둘 다 감당하지 못한다.

export const RECONNECT_BASE_MS = 1_000;
export const RECONNECT_CAP_MS = 30_000;

/**
 * 지수 백오프 + equal jitter.
 * jitter가 없으면 Runner가 죽어 있는 동안 모든 클라이언트가 같은 박자로 재연결을 때린다.
 * full jitter 대신 equal jitter를 쓰는 이유는 최소 대기를 보장해 폭주를 확실히 막기 위해서다.
 */
export function backoffDelay(attempt: number, random: () => number = Math.random): number {
  const step = Math.min(RECONNECT_CAP_MS, RECONNECT_BASE_MS * 2 ** Math.max(0, attempt));
  return Math.round(step / 2 + random() * (step / 2));
}

const SEQUENCE_KEY_PREFIX = "praxis-runner-seq:";

/** localStorage 유사 인터페이스 — 없는 환경(webview 제한 등)에서도 죽지 않게 주입 가능. */
export interface SequenceStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function defaultStorage(): SequenceStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    // Safari의 저장소 차단 등 — 접근 자체가 throw할 수 있다.
    return null;
  }
}

/**
 * Runner endpoint별로 커서를 나눈다. Runner마다 sequence 공간이 독립이라
 * 하나로 섞으면 다른 Runner의 이벤트를 건너뛴다.
 */
function sequenceKey(endpoint: string): string {
  return `${SEQUENCE_KEY_PREFIX}${endpoint}`;
}

export function loadSequence(
  endpoint: string,
  storage: SequenceStorage | null = defaultStorage(),
): number {
  if (!storage) return 0;
  try {
    const raw = storage.getItem(sequenceKey(endpoint));
    if (!raw) return 0;
    const parsed = Number(raw);
    return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : 0;
  } catch {
    return 0;
  }
}

export function saveSequence(
  endpoint: string,
  sequence: number,
  storage: SequenceStorage | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(sequenceKey(endpoint), String(sequence));
  } catch {
    // 저장 실패는 기능을 막지 않는다 — 다음 시작이 replay를 더 볼 뿐이다.
  }
}

/**
 * 서버가 알린 watermark가 우리 커서보다 작으면 Runner의 ledger가 초기화된 것이다
 * (DB 재생성·retention 정리). 이때 커서를 유지하면 이후 이벤트를 전부 건너뛴다.
 */
export function shouldResetCursor(watermark: number, cursor: number): boolean {
  return watermark < cursor;
}

export function isWatermark(value: unknown): value is { kind: "watermark"; sequence: number } {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as { kind?: unknown }).kind === "watermark" &&
    typeof (value as { sequence?: unknown }).sequence === "number"
  );
}
