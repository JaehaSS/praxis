import { describe, expect, it, vi } from "vitest";
import { ConversationQueue, type ConversationQueueDriver, type QueueReadiness } from "./conversation-queue";
import { ConversationSubmitter } from "./conversation-submit";
import type { MessageReceipt } from "./side-question";

const local = { host: "local", id: 7 };
const remote = { host: "ssh", id: 7 };
const receipt = (request_id: string, status: MessageReceipt["status"]): MessageReceipt => ({ request_id, status, error: null });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function setup() {
  const submitter = new ConversationSubmitter();
  const queue = new ConversationQueue(submitter, vi.fn());
  const api = {
    submit: vi.fn(async (id: string, _message: string, _images: string[]) => receipt(id, "accepted")),
    receipt: vi.fn(async (id: string) => receipt(id, "unknown")),
  };
  const driver = {
    readiness: vi.fn(async (): Promise<QueueReadiness> => "ready"),
    admission: vi.fn(() => api),
    onSending: vi.fn(), onAccepted: vi.fn(), onFailed: vi.fn(),
  } satisfies ConversationQueueDriver;
  return { queue, submitter, api, driver };
}

describe("conversation prompt queue", () => {
  it("waits for readiness and delivers one immutable prompt at a time in FIFO order", async () => {
    const { queue, driver, api } = setup();
    const images = ["/capture/a.png"];
    queue.enqueue(local, "first\n\nreference snapshot", images);
    images.push("/capture/new.png");
    queue.enqueue(local, "second", []);
    driver.readiness.mockResolvedValue("busy");
    await queue.flush(local, driver);
    expect(api.submit).not.toHaveBeenCalled();
    expect(queue.snapshot("local:7").items).toHaveLength(2);
    driver.readiness.mockResolvedValue("ready");
    await queue.flush(local, driver);
    expect(api.submit.mock.calls[0].slice(1)).toEqual(["first\n\nreference snapshot", ["/capture/a.png"]]);
    expect(queue.snapshot("local:7").items.map((p) => p.message)).toEqual(["second"]);
    driver.readiness.mockResolvedValue("busy");
    await queue.flush(local, driver);
    expect(api.submit).toHaveBeenCalledTimes(1);
    driver.readiness.mockResolvedValue("ready");
    await queue.flush(local, driver);
    expect(api.submit.mock.calls[1][1]).toBe("second");
    expect(queue.refs()).toEqual([]);
  });

  it("isolates the same task ID on different hosts without depending on selection", async () => {
    const { queue, driver, api } = setup();
    queue.enqueue(local, "local", []);
    queue.enqueue(remote, "remote", []);
    queue.pause("local:7");
    await Promise.all(queue.refs().map((ref) => queue.flush(ref, driver)));
    expect(api.submit.mock.calls.map((call) => call[1])).toEqual(["remote"]);
    expect(driver.admission).toHaveBeenCalledWith(remote);
    expect(queue.has("local:7")).toBe(true);
  });

  it("ignores concurrent flushes and prevents deletion during admission", async () => {
    const { queue, driver, api } = setup();
    const sending = deferred<MessageReceipt>();
    api.submit.mockImplementationOnce(() => sending.promise);
    queue.enqueue(local, "first", []);
    const id = queue.snapshot("local:7").items[0].id;
    const first = queue.flush(local, driver);
    await Promise.resolve();
    await queue.flush(local, driver);
    queue.remove("local:7", id);
    expect(queue.has("local:7")).toBe(true);
    expect(api.submit).toHaveBeenCalledTimes(1);
    sending.resolve(receipt(api.submit.mock.calls[0][0], "accepted"));
    await first;
    expect(queue.has("local:7")).toBe(false);
  });

  it.each(["pause", "remove", "dispose"] as const)("does not send when %s occurs during a readiness check", async (action) => {
    const { queue, driver, api } = setup();
    const checking = deferred<QueueReadiness>();
    driver.readiness.mockReturnValueOnce(checking.promise);
    queue.enqueue(local, "first", []);
    const id = queue.snapshot("local:7").items[0].id;
    const flush = queue.flush(local, driver);
    if (action === "pause") queue.pause("local:7");
    else if (action === "remove") queue.remove("local:7", id);
    else queue.dispose();
    checking.resolve("ready");
    await flush;
    expect(api.submit).not.toHaveBeenCalled();
  });

  it("does not let an old failed check pause a replacement queue", async () => {
    const { queue, driver } = setup();
    const checking = deferred<QueueReadiness>();
    driver.readiness.mockReturnValueOnce(checking.promise);
    queue.enqueue(local, "old", []);
    const flush = queue.flush(local, driver);
    queue.remove("local:7", queue.snapshot("local:7").items[0].id);
    queue.enqueue(local, "replacement", []);
    checking.reject(new Error("offline"));
    await flush;
    expect(queue.snapshot("local:7").paused).toBe(false);
  });

  it("pause during admission consumes only the accepted head and holds the tail", async () => {
    const { queue, driver, api } = setup();
    const sending = deferred<MessageReceipt>();
    api.submit.mockImplementationOnce(() => sending.promise);
    queue.enqueue(local, "first", []);
    queue.enqueue(local, "second", []);
    const flush = queue.flush(local, driver);
    await Promise.resolve();
    queue.pause("local:7", "interrupted");
    sending.resolve(receipt(api.submit.mock.calls[0][0], "accepted"));
    await flush;
    await queue.flush(local, driver);
    expect(queue.snapshot("local:7")).toMatchObject({ paused: true, reason: "interrupted", items: [{ message: "second" }] });
    expect(api.submit).toHaveBeenCalledTimes(1);
  });

  it("retains a rejected prompt and attachments, allowing deletion after failure", async () => {
    const { queue, driver, api } = setup();
    api.submit.mockImplementationOnce(async (id) => receipt(id, "failed"));
    queue.enqueue(local, "first", ["image.png"]);
    await queue.flush(local, driver);
    expect(queue.snapshot("local:7")).toMatchObject({ paused: true, items: [{ message: "first", images: ["image.png"], uncertain: false }] });
    queue.remove("local:7", queue.snapshot("local:7").items[0].id);
    expect(queue.has("local:7")).toBe(false);
  });

  it("holds an uncertain request and recovers its receipt after the task has ended without resending", async () => {
    const { queue, driver, api } = setup();
    api.submit.mockRejectedValueOnce(new Error("reply lost"));
    queue.enqueue(local, "first", ["image.png"]);
    queue.enqueue(local, "second", []);
    await queue.flush(local, driver);
    const head = queue.snapshot("local:7").items[0];
    expect(head.uncertain).toBe(true);
    queue.remove("local:7", head.id);
    expect(queue.snapshot("local:7").items).toHaveLength(2);
    await queue.flush(local, driver);
    expect(api.submit).toHaveBeenCalledTimes(1);
    driver.readiness.mockResolvedValue({ blocked: "ended" });
    api.receipt.mockImplementation(async (id) => receipt(id, "accepted"));
    queue.resume("local:7");
    await queue.flush(local, driver);
    expect(api.submit).toHaveBeenCalledTimes(1);
    expect(api.receipt.mock.calls.every(([id]) => id === api.submit.mock.calls[0][0])).toBe(true);
    expect(queue.snapshot("local:7").items.map((item) => item.message)).toEqual(["second"]);
    await queue.flush(local, driver);
    expect(queue.snapshot("local:7").reason).toBe("ended");
  });

  it("retries an unresolved request with the same ID and payload before the tail", async () => {
    const { queue, driver, api } = setup();
    api.submit.mockRejectedValueOnce(new Error("reply lost"));
    queue.enqueue(local, "first", ["image.png"]);
    queue.enqueue(local, "second", []);
    await queue.flush(local, driver);
    queue.resume("local:7");
    await queue.flush(local, driver);
    expect(api.submit.mock.calls[1]).toEqual(api.submit.mock.calls[0]);
    expect(queue.snapshot("local:7").items.map((item) => item.message)).toEqual(["second"]);
  });

  it("does not overtake a direct request with an unresolved receipt", async () => {
    const { queue, submitter, driver, api } = setup();
    api.submit.mockRejectedValueOnce(new Error("reply lost"));
    await expect(submitter.send("local:7", api, "direct", [])).rejects.toThrow();
    queue.enqueue(local, "queued", []);
    await queue.flush(local, driver);
    expect(api.submit).toHaveBeenCalledTimes(1);
    expect(queue.snapshot("local:7").paused).toBe(true);
    expect(queue.snapshot("local:7").items[0].uncertain).toBe(false);
  });
});
