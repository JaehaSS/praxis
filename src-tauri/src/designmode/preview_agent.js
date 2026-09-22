(function () {
  "use strict";

  function snapshot() {
    return document.documentElement ? document.documentElement.outerHTML : "";
  }

  function nativeValueSetter(element) {
    var prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    return Object.getOwnPropertyDescriptor(prototype, "value").set;
  }

  async function execute(action) {
    try {
      var before = snapshot();
      var element = await waitForSelector(action);
      if (!element) return { status: "ACTION_TIMEOUT", changed: false };
      if (action.kind === "wait") return { status: "OK", changed: false };
      if (action.kind === "fill") fill(element, action.text);
      if (action.kind === "click") click(element);
      await Promise.resolve();
      return { status: "OK", changed: before !== snapshot() };
    } catch (error) {
      return { status: "JS_EXCEPTION", changed: false, message: String(error.message || error) };
    }
  }

  function fill(element, text) {
    nativeValueSetter(element).call(element, text);
    element.dispatchEvent(new Event("input", { bubbles: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function click(element) {
    element.click();
  }

  function timeoutMs(action) {
    var requested = Number(action.timeoutMs);
    return Math.min(2000, Number.isFinite(requested) && requested > 0 ? requested : 2000);
  }

  function pollingBudgetMs(action) {
    var timeout = timeoutMs(action);
    // Reserve a small scheduling margin so the public two-second maximum is
    // observed even when the final timer callback arrives late.
    return Math.max(0, timeout - Math.min(75, timeout / 4));
  }

  function sleep(ms) {
    return new Promise(function (resolve) { setTimeout(resolve, ms); });
  }

  async function waitForSelector(action) {
    var deadline = Date.now() + pollingBudgetMs(action);
    while (Date.now() <= deadline) {
      var element = document.querySelector(action.selector);
      if (element) return element;
      await sleep(Math.min(20, Math.max(0, deadline - Date.now())));
    }
    return null;
  }

  // 웹뷰에서 계산한다 — loopback http도 secure context라 crypto.subtle을 쓸 수 있다.
  async function sha256Hex(text) {
    var digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
    return Array.prototype.map
      .call(new Uint8Array(digest), function (byte) { return byte.toString(16).padStart(2, "0"); })
      .join("");
  }

  function submitResult(result) {
    var internals = window.__TAURI_INTERNALS__;
    if (!internals || !internals.invoke) return Promise.reject(new Error("TAURI_IPC_UNAVAILABLE"));
    return internals.invoke("plugin:preview-bridge|submit_result", { result: result });
  }

  window.__praxisPreviewAgent = {
    execute: execute,
    sha256Hex: sha256Hex,
    submitResult: submitResult,
    phaseOneProbe: function () {
      return { csp: "default-src 'self'; connect-src 'none'", transport: "ipc", fallbackActive: false, requiresPackagedApp: true };
    }
  };
})();
