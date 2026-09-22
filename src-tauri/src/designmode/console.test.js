// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, it, expect, beforeEach, afterEach } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
const load = (name) => readFileSync(path.join(here, name), "utf-8");
const [execSource, consoleSource] = [load("exec.js"), load("console.js")];

const originals = { ...console };
beforeEach(() => {
  document.body.innerHTML = "";
  delete window.__praxisPreviewConsole;
  new Function(execSource)();
  new Function(consoleSource)();
});
afterEach(() => { Object.assign(console, originals); });

const run = (cmd) => window.__praxisPreviewExec.run(cmd).then(JSON.parse);

describe("console capture", () => {
  it("records every level and returns them without a snapshot", async () => {
    console.log("하나");
    console.warn("둘");
    console.error("셋");
    const res = await run({ op: "console" });
    expect(res).toMatchObject({ ok: true, dropped: 0 });
    expect(res.snapshot).toBeUndefined();
    expect(res.entries.map((e) => [e.level, e.text])).toEqual([
      ["log", "하나"], ["warn", "둘"], ["error", "셋"]
    ]);
    expect(res.entries[0].ts).toBeGreaterThan(0);
  });

  it("still calls the original console method", async () => {
    const seen = [];
    delete window.__praxisPreviewConsole;
    console.info = (...args) => seen.push(args.join(" "));
    new Function(consoleSource)();
    console.info("본문", 2);
    expect(seen).toEqual(["본문 2"]);
    expect((await run({ op: "console" })).entries.at(-1).text).toBe("본문 2");
  });

  it("joins arguments and stringifies objects and errors safely", async () => {
    const cyclic = {}; cyclic.self = cyclic;
    console.log("a", { b: 1 }, new TypeError("펑"), cyclic);
    const res = await run({ op: "console" });
    expect(res.entries[0].text).toBe(`a {"b":1} TypeError: 펑 [object Object]`);
  });

  it("clips a long line to 1024 characters", async () => {
    console.log("가".repeat(2000));
    const res = await run({ op: "console" });
    expect(res.entries[0].text.length).toBe(1024);
  });

  it("keeps the last 200 entries and counts the rest as dropped", async () => {
    for (let i = 0; i < 205; i++) console.log(`m${i}`);
    const res = await run({ op: "console" });
    expect(res.entries.length).toBe(200);
    expect(res.dropped).toBe(5);
    expect(res.entries[0].text).toBe("m5");
  });

  it("empties the buffer only after producing the result", async () => {
    console.log("마지막");
    const first = await run({ op: "console", clear: true });
    expect(first.entries.length).toBe(1);
    const second = await run({ op: "console" });
    expect(second).toMatchObject({ entries: [], dropped: 0 });
  });

  it("records window errors and unhandled rejections", async () => {
    window.dispatchEvent(new ErrorEvent("error", { message: "터짐" }));
    const event = new Event("unhandledrejection");
    event.reason = new Error("거절");
    window.dispatchEvent(event);
    const res = await run({ op: "console" });
    expect(res.entries.map((e) => [e.level, e.text])).toEqual([
      ["error", "터짐"], ["error", "Unhandled rejection: Error: 거절"]
    ]);
  });

  it("exposes the buffer for tests and is idempotent when loaded twice", () => {
    console.log("한 번");
    new Function(consoleSource)();
    console.log("두 번");
    expect(window.__praxisPreviewConsole.entries().map((e) => e.text)).toEqual(["한 번", "두 번"]);
    window.__praxisPreviewConsole.clear();
    expect(window.__praxisPreviewConsole.entries()).toEqual([]);
  });

  it("survives a console object missing a level", () => {
    delete window.__praxisPreviewConsole;
    const debug = console.debug;
    console.debug = undefined;
    try {
      expect(() => new Function(consoleSource)()).not.toThrow();
    } finally {
      console.debug = debug;
    }
  });
});
