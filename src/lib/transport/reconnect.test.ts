import { describe, expect, it } from "vitest";
import {
  backoffDelay,
  isWatermark,
  loadSequence,
  RECONNECT_BASE_MS,
  RECONNECT_CAP_MS,
  saveSequence,
  shouldResetCursor,
  type SequenceStorage,
} from "./reconnect";

function memoryStorage(initial: Record<string, string> = {}): SequenceStorage {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => void map.set(key, value),
  };
}

describe("backoffDelay", () => {
  it("지수적으로 늘고 상한에서 멈춘다", () => {
    // random=1이면 상한값(=step)을 그대로 쓴다.
    expect(backoffDelay(0, () => 1)).toBe(RECONNECT_BASE_MS);
    expect(backoffDelay(1, () => 1)).toBe(2 * RECONNECT_BASE_MS);
    expect(backoffDelay(3, () => 1)).toBe(8 * RECONNECT_BASE_MS);
    expect(backoffDelay(20, () => 1)).toBe(RECONNECT_CAP_MS);
  });

  it("jitter가 최소 대기를 보장한다", () => {
    // random=0이어도 절반은 남아, 즉시 재연결로 폭주하지 않는다.
    expect(backoffDelay(0, () => 0)).toBe(RECONNECT_BASE_MS / 2);
    expect(backoffDelay(20, () => 0)).toBe(RECONNECT_CAP_MS / 2);
  });

  it("음수 시도는 0으로 취급한다", () => {
    expect(backoffDelay(-5, () => 1)).toBe(RECONNECT_BASE_MS);
  });

  it("모든 지연이 상한을 넘지 않는다", () => {
    for (let attempt = 0; attempt < 40; attempt += 1) {
      const delay = backoffDelay(attempt, () => 0.999);
      expect(delay).toBeLessThanOrEqual(RECONNECT_CAP_MS);
      expect(delay).toBeGreaterThan(0);
    }
  });
});

describe("sequence 커서", () => {
  it("endpoint별로 분리 저장한다", () => {
    // 섞이면 다른 Runner의 이벤트를 건너뛴다.
    const storage = memoryStorage();
    saveSequence("http://a:1", 10, storage);
    saveSequence("http://b:2", 99, storage);
    expect(loadSequence("http://a:1", storage)).toBe(10);
    expect(loadSequence("http://b:2", storage)).toBe(99);
  });

  it("없거나 망가진 값은 0으로 떨어진다", () => {
    const storage = memoryStorage({ "praxis-runner-seq:x": "not-a-number" });
    expect(loadSequence("x", storage)).toBe(0);
    expect(loadSequence("없음", storage)).toBe(0);
    expect(loadSequence("neg", memoryStorage({ "praxis-runner-seq:neg": "-3" }))).toBe(0);
  });

  it("저장소가 없어도 죽지 않는다", () => {
    expect(loadSequence("x", null)).toBe(0);
    expect(() => saveSequence("x", 1, null)).not.toThrow();
  });

  it("저장소가 던져도 죽지 않는다", () => {
    const hostile: SequenceStorage = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(loadSequence("x", hostile)).toBe(0);
    expect(() => saveSequence("x", 1, hostile)).not.toThrow();
  });
});

describe("shouldResetCursor", () => {
  it("watermark가 커서보다 작으면 ledger 초기화로 본다", () => {
    expect(shouldResetCursor(5, 100)).toBe(true);
    expect(shouldResetCursor(0, 1)).toBe(true);
  });

  it("정상 상황에서는 유지한다", () => {
    expect(shouldResetCursor(100, 100)).toBe(false);
    expect(shouldResetCursor(200, 100)).toBe(false);
    expect(shouldResetCursor(0, 0)).toBe(false);
  });
});

describe("isWatermark", () => {
  it("watermark 메시지만 인식한다", () => {
    expect(isWatermark({ kind: "watermark", sequence: 3 })).toBe(true);
    expect(isWatermark({ kind: "watermark", sequence: "3" })).toBe(false);
    expect(isWatermark({ kind: "state", sequence: 3, task_id: 1 })).toBe(false);
    expect(isWatermark(null)).toBe(false);
    expect(isWatermark("watermark")).toBe(false);
  });
});
