// @vitest-environment jsdom
//
// Phase 1은 실제 WKWebView live probe와 단위 테스트를 혼동하지 않는다. 이 파일은
// injected action의 DOM 계약과 strict-CSP probe marker만 고정한다.
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const here = path.dirname(fileURLToPath(import.meta.url));
const source = readFileSync(path.join(here, "preview_agent.js"), "utf-8");

describe("preview agent injected action bridge", () => {
  let agent;
  let root;

  beforeAll(() => {
    // eslint-disable-next-line no-new-func
    new Function(source)();
    agent = window.__praxisPreviewAgent;
  });

  beforeEach(() => {
    document.body.innerHTML = '<div id="root"></div>';
    root = createRoot(document.getElementById("root"));
  });

  afterEach(async () => {
    await act(async () => root.unmount());
  });

  async function execute(action) {
    let result;
    await act(async () => {
      result = await agent.execute(action);
    });
    return result;
  }

  it("fills a React controlled input with its native setter and reports DOM change", async () => {
    function Fixture() {
      const [value, setValue] = React.useState("before");
      return React.createElement("input", { id: "controlled", value, onChange: (event) => setValue(event.target.value) });
    }
    await act(async () => root.render(React.createElement(Fixture)));

    const result = await execute({ kind: "fill", selector: "#controlled", text: "after" });
    expect(result).toMatchObject({ status: "OK", changed: true });
    expect(document.getElementById("controlled").value).toBe("after");
  });

  it("reports changed true only when a button click changes the observed DOM", async () => {
    function Fixture() {
      const [on, setOn] = React.useState(false);
      return React.createElement(React.Fragment, null,
        React.createElement("button", { id: "toggle", onClick: () => setOn(true) }, on ? "on" : "off"),
        React.createElement("button", { id: "noop" }, "noop"));
    }
    await act(async () => root.render(React.createElement(Fixture)));

    expect(await execute({ kind: "click", selector: "#toggle" })).toMatchObject({ status: "OK", changed: true });
    expect(document.getElementById("toggle").textContent).toBe("on");
    expect(await execute({ kind: "click", selector: "#noop" })).toMatchObject({ status: "OK", changed: false });
  });

  it("classifies injected exceptions within 500ms without generic eval", async () => {
    const querySelector = document.querySelector.bind(document);
    document.querySelector = () => { throw new Error("fixture query failure"); };
    const started = performance.now();
    await expect(execute({ kind: "click", selector: "#anything" })).resolves.toMatchObject({ status: "JS_EXCEPTION" });
    expect(performance.now() - started).toBeLessThan(500);
    document.querySelector = querySelector;
  });

  it("waits for a selector that appears before the action deadline", async () => {
    setTimeout(() => {
      const delayed = document.createElement("div");
      delayed.id = "later";
      document.body.appendChild(delayed);
    }, 60);

    const started = performance.now();
    await expect(execute({ kind: "wait", selector: "#later", timeoutMs: 250 })).resolves.toMatchObject({ status: "OK" });
    expect(performance.now() - started).toBeGreaterThanOrEqual(50);
  });

  it("uses the configured two-second host timeout instead of a short internal cap", async () => {
    const started = performance.now();
    await expect(execute({ kind: "wait", selector: "#missing", timeoutMs: 2_000 })).resolves.toMatchObject({ status: "ACTION_TIMEOUT" });
    const elapsed = performance.now() - started;
    expect(elapsed).toBeGreaterThanOrEqual(1_800);
    expect(elapsed).toBeLessThan(2_000);
  });

  it("hashes text with SHA-256 as lowercase hex", async () => {
    await expect(agent.sha256Hex("abc")).resolves.toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
    await expect(agent.sha256Hex("")).resolves.toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
    await expect(agent.sha256Hex("한글 미리보기")).resolves.toBe(
      "a06b140bb82cd2207b0ae7dc6c9f98e63b604d8a70bbc029e68b50499fa5f043",
    );
  });

  it("marks strict CSP as a packaged-app live probe and keeps fallback inactive after IPC success", () => {
    expect(agent.phaseOneProbe()).toEqual({
      csp: "default-src 'self'; connect-src 'none'",
      transport: "ipc",
      fallbackActive: false,
      requiresPackagedApp: true,
    });
  });
});
