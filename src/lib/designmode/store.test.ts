import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearCaptures, getCaptures, pushCapture, removeCapture, subscribeCaptures } from "./store";
import type { DesignCaptureRecord } from "./types";

function record(id: string): DesignCaptureRecord {
  return {
    id,
    task_id: 1,
    source: "preview",
    outer_html: "<div/>",
    computed_css: {},
    bounding_rect: { x: 0, y: 0, width: 0, height: 0 },
    captured_at: 0,
    image_path: null,
    file_path: null,
    selection_text: null,
    selection_start_line: null,
    selection_end_line: null,
  };
}

describe("designmode store", () => {
  const taskId = 1;

  beforeEach(() => {
    clearCaptures(taskId);
    clearCaptures(2);
  });

  it("pushCapture로 추가한 캡처를 getCaptures가 반환한다", () => {
    pushCapture(taskId, record("1"));
    expect(getCaptures(taskId).map((capture) => capture.id)).toEqual(["1"]);
  });

  it("removeCapture는 지정한 id만 제거한다", () => {
    pushCapture(taskId, record("1"));
    pushCapture(taskId, record("2"));
    removeCapture(taskId, "1");
    expect(getCaptures(taskId).map((capture) => capture.id)).toEqual(["2"]);
  });

  it("clearCaptures는 전체를 비운다", () => {
    pushCapture(taskId, record("1"));
    clearCaptures(taskId);
    expect(getCaptures(taskId)).toEqual([]);
  });

  it("subscribeCaptures는 변경마다 최신 목록으로 호출된다", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeCaptures(taskId, listener);
    pushCapture(taskId, record("1"));
    expect(listener).toHaveBeenCalledWith([expect.objectContaining({ id: "1" })]);
    unsubscribe();
    pushCapture(taskId, record("2"));
    expect(listener).toHaveBeenCalledTimes(1); // 해제 후에는 더 호출되지 않는다.
  });

  it("같은 repo여도 다른 task id의 캡처는 서로 격리된다", () => {
    pushCapture(2, record("x"));
    expect(getCaptures(taskId)).toEqual([]);
  });
});
