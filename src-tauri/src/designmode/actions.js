// 사전검사·click/fill/press_key·wait_for. exec.js가 만든 op 레지스트리에 등록한다.
(function () {
  "use strict";

  var exec = window.__praxisPreviewExec;
  var ctx = exec._internal;

  function isVisible(el) {
    if (!ctx.layout().rects(el).length) return false;
    var style = window.getComputedStyle(el);
    return !style || (style.visibility !== "hidden" && style.display !== "none");
  }

  function isDisabled(el) {
    if (el.disabled === true) return true;
    if (el.getAttribute("aria-disabled") === "true") return true;
    return !!(el.closest && el.closest("fieldset[disabled]"));
  }

  // 좌표가 화면 밖이면 elementFromPoint가 null을 준다 — 가림이 아니라 보이지 않는 것이다.
  function hitTest(el) {
    var rect = ctx.layout().rect(el);
    var hit = ctx.layout().elementAt(rect.left + rect.width / 2, rect.top + rect.height / 2);
    if (!hit) return { error: "not_visible" };
    if (hit === el || hit.contains(el) || el.contains(hit)) return null;
    return { error: "obscured", obscured_by: ctx.describe(hit) };
  }

  function precheck(ref) {
    var found = ctx.resolve(ref);
    if (found.error) return found;
    if (!isVisible(found.el)) return { error: "not_visible" };
    if (isDisabled(found.el)) return { error: "disabled" };
    return hitTest(found.el) || found;
  }

  var UNFILLABLE_TYPE = { button: 1, submit: 1, checkbox: 1, radio: 1 };

  function isFillable(el) {
    if (el.isContentEditable) return true;
    var tag = el.tagName.toLowerCase();
    if (tag === "textarea" || tag === "select") return true;
    return tag === "input" && !UNFILLABLE_TYPE[(el.type || "text").toLowerCase()];
  }

  function highlightStyle(rect) {
    var custom = window.__praxisDesignMode && window.__praxisDesignMode._internal &&
      window.__praxisDesignMode._internal.highlightStyleFor;
    if (custom) return custom(rect);
    return {
      position: "fixed", left: rect.left + "px", top: rect.top + "px",
      width: rect.width + "px", height: rect.height + "px",
      pointerEvents: "none", zIndex: "2147483647", outline: "2px solid #14b8a6"
    };
  }

  // 박스는 해시 밖에 둔다 — before 해시 뒤에 넣고, after 해시 직전에 잠시 떼었다가 되돌린다.
  // settle이 600ms를 넘겨 박스가 먼저 사라져도 changed가 오탐되지 않는다.
  function showHighlight(rect) {
    var box = document.createElement("div");
    box.setAttribute("data-praxis-highlight", "");
    var style = highlightStyle(rect);
    for (var key in style) {
      if (Object.prototype.hasOwnProperty.call(style, key)) box.style[key] = style[key];
    }
    var shown = true;
    document.body.appendChild(box);
    setTimeout(function () { shown = false; box.remove(); }, 600);
    return {
      detach: function () { box.remove(); },
      restore: function () { if (shown) document.body.appendChild(box); }
    };
  }

  var CLICK_SEQUENCE = ["pointerdown", "mousedown", "pointerup", "mouseup", "click"];

  function doClick(el, _cmd, rect) {
    var Ctor = typeof PointerEvent === "function" ? PointerEvent : MouseEvent;
    if (el.focus) el.focus();
    for (var i = 0; i < CLICK_SEQUENCE.length; i++) {
      el.dispatchEvent(new Ctor(CLICK_SEQUENCE[i], {
        bubbles: true, cancelable: true, composed: true,
        clientX: rect.left + rect.width / 2, clientY: rect.top + rect.height / 2
      }));
    }
  }

  function valueSetter(el) {
    var proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype
      : el instanceof HTMLSelectElement ? HTMLSelectElement.prototype
        : HTMLInputElement.prototype;
    return Object.getOwnPropertyDescriptor(proto, "value").set;
  }

  // React는 네이티브 setter를 후킹해 값 변경을 감지한다 — el.value = text는 다음 렌더에서 되돌아간다.
  function doFill(el, cmd) {
    var text = cmd.text == null ? "" : String(cmd.text);
    if (el.focus) el.focus();
    if (el.isContentEditable) el.textContent = text;
    else valueSetter(el).call(el, text);
    var InputCtor = typeof InputEvent === "function" ? InputEvent : Event;
    el.dispatchEvent(new InputCtor("input", { bubbles: true, inputType: "insertText" }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function codeFor(key) {
    if (Array.from(key).length !== 1) return key;
    if (/[a-zA-Z]/.test(key)) return "Key" + key.toUpperCase();
    if (/[0-9]/.test(key)) return "Digit" + key;
    return "";
  }

  function keyEvent(type, key) {
    return new KeyboardEvent(type, {
      key: key, code: codeFor(key), bubbles: true, cancelable: true, composed: true
    });
  }

  function submitForm(el) {
    var form = el.form || (el.closest ? el.closest("form") : null);
    if (!form) return;
    if (form.requestSubmit) return form.requestSubmit();
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  }

  function doPressKey(el, cmd) {
    var key = String(cmd.key || "");
    var accepted = el.dispatchEvent(keyEvent("keydown", key));
    if (Array.from(key).length === 1) el.dispatchEvent(keyEvent("keypress", key));
    el.dispatchEvent(keyEvent("keyup", key));
    if (key === "Enter" && accepted) submitForm(el);
  }

  var ACTIONS = { click: doClick, fill: doFill, press_key: doPressKey };

  function targetFor(cmd) {
    // ref 없는 키 입력은 포커스를 따라간다 — 좌표가 없으니 가시성·가림 검사는 걸지 않는다.
    if (cmd.op === "press_key" && !cmd.ref) return { el: document.activeElement || document.body };
    return precheck(cmd.ref);
  }

  async function act(cmd) {
    var target = targetFor(cmd);
    if (target.error) return Object.assign({ ok: false }, target);
    if (cmd.op === "fill" && !isFillable(target.el)) return { ok: false, error: "not_fillable" };
    var rect = ctx.layout().rect(target.el);
    var before = ctx.domHash();
    var highlight = showHighlight(rect);
    ACTIONS[cmd.op](target.el, cmd, rect);
    await ctx.settle();
    highlight.detach();
    var changed = before !== ctx.domHash();
    highlight.restore();
    return { ok: true, changed: changed, target: ctx.describe(target.el) };
  }

  // innerText는 jsdom에 없다 — 레이아웃이 없는 곳에서는 textContent로 떨어진다.
  function pageText() {
    var body = document.body;
    if (!body) return "";
    return body.innerText === undefined || body.innerText === null ? (body.textContent || "") : body.innerText;
  }

  function isSatisfied(cmd) {
    var text = pageText();
    if (cmd.text && text.indexOf(cmd.text) === -1) return false;
    return !(cmd.gone && text.indexOf(cmd.gone) !== -1);
  }

  var POLL_MS = 100;
  var MAX_TIMEOUT_MS = 60000;

  // 먼저 한 번 보고 나서 폴링한다 — 이미 만족된 조건에 100ms를 쓰지 않는다.
  async function waitFor(cmd) {
    if (!cmd.text && !cmd.gone) return { ok: false, error: "invalid_argument" };
    var budget = Math.min(MAX_TIMEOUT_MS, Math.max(0, Number(cmd.timeout_ms) || 0));
    var started = Date.now();
    for (;;) {
      if (isSatisfied(cmd)) return { ok: true, satisfied: true, elapsed_ms: Date.now() - started };
      if (Date.now() - started >= budget) return { ok: true, satisfied: false, elapsed_ms: Date.now() - started };
      await new Promise(function (done) { setTimeout(done, POLL_MS); });
    }
  }

  exec._registerOp("click", act);
  exec._registerOp("fill", act);
  exec._registerOp("press_key", act);
  exec._registerOp("wait_for", waitFor);
})();
