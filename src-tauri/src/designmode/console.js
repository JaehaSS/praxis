// 페이지 콘솔 링버퍼. initialization_script로 들어가므로 페이지 스크립트보다 먼저 후킹한다.
(function () {
  "use strict";

  // 두 번 로드되면 원본 console을 이미 감싼 함수로 다시 감싸 로그가 겹친다.
  if (window.__praxisPreviewConsole) return;

  var MAX_ENTRIES = 200;
  var MAX_TEXT = 1024;
  var LEVELS = ["log", "info", "warn", "error", "debug"];

  var entries = [];
  var dropped = 0;

  function stringify(value) {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.name + ": " + value.message;
    try {
      var json = JSON.stringify(value);
      return json === undefined ? String(value) : json;
    } catch (error) {
      return String(value);
    }
  }

  function textOf(args) {
    var parts = [];
    for (var i = 0; i < args.length; i++) parts.push(stringify(args[i]));
    return parts.join(" ").slice(0, MAX_TEXT);
  }

  function record(level, text) {
    entries.push({ level: level, text: text.slice(0, MAX_TEXT), ts: Date.now() });
    while (entries.length > MAX_ENTRIES) {
      entries.shift();
      dropped += 1;
    }
  }

  function hook(level) {
    var original = console[level];
    if (typeof original !== "function") return;
    console[level] = function () {
      try {
        record(level, textOf(arguments));
      } catch (error) {
        /* 기록 실패가 페이지의 console 호출을 깨뜨리지 않게 한다. */
      }
      return original.apply(console, arguments);
    };
  }

  if (typeof console === "object" && console) {
    for (var i = 0; i < LEVELS.length; i++) hook(LEVELS[i]);
  }

  window.addEventListener("error", function (event) {
    record("error", event && event.message ? String(event.message) : stringify(event && event.error));
  });

  window.addEventListener("unhandledrejection", function (event) {
    record("error", "Unhandled rejection: " + stringify(event && event.reason));
  });

  function clear() {
    entries = [];
    dropped = 0;
  }

  // 결과를 만든 뒤에 비운다 — clear를 준 호출도 그때까지 쌓인 항목을 받아 간다.
  async function readConsole(cmd) {
    var result = { ok: true, snapshot: false, entries: entries.slice(), dropped: dropped };
    if (cmd && cmd.clear) clear();
    return result;
  }

  window.__praxisPreviewExec._registerOp("console", readConsole);
  window.__praxisPreviewConsole = {
    entries: function () { return entries.slice(); },
    clear: clear
  };
})();
