import { describe, expect, it } from "vitest";
import {
  MIN_EDITOR_HEIGHT,
  maxDockHeight,
  readStoredDockHeight,
  writeStoredDockHeight,
  type DockHeightStorage,
} from "./terminal-dock-height";

function memoryStorage(): DockHeightStorage {
  const values = new Map<string, string>();
  return {
    getItem: (key: string): string | null => values.get(key) ?? null,
    setItem: (key: string, value: string): void => {
      values.set(key, value);
    },
  };
}

describe("terminal dock height", () => {
  it("저장한 적이 없으면 기본 높이로 연다", () => {
    expect(readStoredDockHeight(memoryStorage())).toBe(260);
  });

  it("조절한 높이를 다음에 그대로 복원한다", () => {
    const storage = memoryStorage();

    writeStoredDockHeight(storage, 340);

    expect(readStoredDockHeight(storage)).toBe(340);
  });

  it("상한은 창 높이에서 에디터 최소 높이를 뺀 값이라 창이 클수록 높아진다", () => {
    const storage = memoryStorage();

    writeStoredDockHeight(storage, 5000, 1000);

    expect(maxDockHeight(1000)).toBe(1000 - MIN_EDITOR_HEIGHT);
    expect(readStoredDockHeight(storage, 1000)).toBe(1000 - MIN_EDITOR_HEIGHT);
  });

  it("창이 아주 낮으면 상한보다 최소 높이(120)가 우선한다", () => {
    const storage = memoryStorage();

    writeStoredDockHeight(storage, 5000, 240);

    expect(readStoredDockHeight(storage, 240)).toBe(120);
  });

  it("낮은 창에서 잘린 값이 큰 창의 복원까지 낮추지는 않는다", () => {
    const storage = memoryStorage();

    writeStoredDockHeight(storage, 600, 1000);

    expect(readStoredDockHeight(storage, 500)).toBe(500 - MIN_EDITOR_HEIGHT);
    expect(readStoredDockHeight(storage, 1000)).toBe(600);
  });

  it("깨진 저장값은 기본 높이로 되돌린다", () => {
    const storage = memoryStorage();
    storage.setItem("praxis-terminal-dock-height", "nope");

    expect(readStoredDockHeight(storage)).toBe(260);
  });
});
