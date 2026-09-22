(function () {
  "use strict";

  var generation = 0;
  var refs = new Map();

  var ROLE = {
    a: "link", button: "button", input: "textbox", textarea: "textbox", select: "combobox",
    h1: "heading", h2: "heading", h3: "heading", img: "img", label: "text"
  };

  // 코드 유닛이 아니라 코드 포인트로 자른다 — UTF-16 대리 쌍이 반으로 갈리면 깨진 문자가 남는다.
  function clip(str) {
    return Array.from(str).slice(0, 80).join("");
  }

  function nameOf(el) {
    return clip(el.getAttribute("aria-label") || el.getAttribute("alt") || el.getAttribute("title") ||
      (el.textContent || "").trim());
  }

  function roleOf(el) {
    return el.getAttribute("role") || ROLE[el.tagName.toLowerCase()] || null;
  }

  // 비밀 입력은 종류만 적고 값은 절대 싣지 않는다 — 스냅샷은 호스트로 나가는 payload다.
  function secretKind(el) {
    if (el.type === "password") return "password";
    if (el.type === "hidden") return "hidden";
    if ((el.getAttribute("autocomplete") || "").toLowerCase() === "one-time-code") return "otp";
    return null;
  }

  function lineFor(el, role, ref) {
    var line = "- " + role + " \"" + nameOf(el) + "\"";
    if (ref) line += " [ref=" + ref + "]";
    if (/^h[1-6]$/i.test(el.tagName)) line += " level=" + el.tagName[1];
    var secret = secretKind(el);
    if (secret) return line + " type=" + secret;
    if ("value" in el && el.tagName !== "BUTTON") line += " value=\"" + clip(el.value || "") + "\"";
    return line;
  }

  // 방문한 요소를 한 줄로 적는다. 노드 상한에 닿으면 순회 전체를 멈춘다.
  function visit(el, role, depth, maxNodes, state) {
    if (state.count >= maxNodes) {
      state.truncated = true;
      state.stopped = true;
      return false;
    }
    state.count += 1;
    var ref = role === "heading" || role === "text" ? null : "s" + generation + "e" + state.count;
    if (ref) refs.set(ref, el);
    state.lines.push("  ".repeat(depth) + lineFor(el, role, ref));
    return true;
  }

  // 깊이 초과는 그 가지만 접는다 — 형제 노드는 계속 본다. 순회를 통째로 멈추는 것은 노드 상한뿐이다.
  function walk(el, depth, maxDepth, maxNodes, state) {
    if (state.stopped) return;
    if (depth > maxDepth) {
      state.truncated = true;
      return;
    }
    var role = roleOf(el);
    if (role && !visit(el, role, depth, maxNodes, state)) return;
    for (var i = 0; i < el.children.length; i++) walk(el.children[i], depth + 1, maxDepth, maxNodes, state);
  }

  // ref는 DOM에 쓰지 않고 in-memory Map에만 둔다 — 페이지를 오염시키지 않기 위해서다.
  // 스냅샷마다 Map을 새로 만들므로 이전 세대의 ref는 자동으로 stale이 된다.
  function snapshot(opts) {
    opts = opts || {};
    var maxDepth = opts.maxDepth || 12;
    var maxNodes = opts.maxNodes || 800;
    generation += 1;
    refs = new Map();
    var state = { lines: [], count: 0, truncated: false, stopped: false };
    walk(document.body, 0, maxDepth, maxNodes, state);
    // bytes는 text만 잰다 — 에이전트 컨텍스트에 쌓이는 것이 트리 본문이고, 그것이 계측의 대상이다.
    var text = state.lines.join("\n");
    return { generation: generation, text: text, truncated: state.truncated, url: location.href,
      nodes: state.count, bytes: byteLength(text) };
  }

  function resolve(ref) {
    var el = refs.get(ref);
    if (!el) return { error: "stale_ref" };
    if (!el.isConnected) return { error: "stale_ref" };
    return { el: el };
  }

  // 레이아웃 조회를 한 곳에 모은다 — jsdom에는 레이아웃이 없어 테스트가 _setLayout으로 갈아끼운다.
  var DEFAULT_LAYOUT = {
    rects: function (el) { return el.getClientRects(); },
    rect: function (el) { return el.getBoundingClientRect(); },
    elementAt: function (x, y) { return document.elementFromPoint(x, y); }
  };
  var layout = DEFAULT_LAYOUT;

  // 테스트 전용. 프로덕션 경로에서는 아무도 부르지 않는다.
  function setLayout(partial) {
    layout = Object.assign({}, DEFAULT_LAYOUT, partial || {});
  }

  function describe(el) {
    return (roleOf(el) || el.tagName.toLowerCase()) + " \"" + nameOf(el) + "\"";
  }

  // FNV-1a 32비트. 동기 해시라 액션 전후를 같은 틱에서 비교할 수 있다.
  function domHash() {
    var text = (document.documentElement ? document.documentElement.outerHTML : "") + "\n" + location.href;
    var hash = 0x811c9dc5;
    for (var i = 0; i < text.length; i++) {
      hash ^= text.charCodeAt(i);
      hash = (hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24))) >>> 0;
    }
    return hash;
  }

  // 가려진 웹뷰는 rAF를 멈춘다 — 프레임을 기다리되 상한을 둬서 명령 타임아웃으로 새지 않게 한다.
  var FRAME_FALLBACK_MS = 100;

  function nextFrame() {
    return new Promise(function (done) {
      var settled = false;
      function once() { if (!settled) { settled = true; done(); } }
      if (typeof requestAnimationFrame === "function") requestAnimationFrame(once);
      setTimeout(once, typeof requestAnimationFrame === "function" ? FRAME_FALLBACK_MS : 0);
    });
  }

  // 렌더 두 프레임 + 짧은 여유. 액션의 효과가 DOM에 닿기 전에 해시를 읽지 않기 위해서다.
  function settle() {
    return nextFrame().then(nextFrame).then(function () {
      return new Promise(function (done) { setTimeout(done, 150); });
    });
  }

  // 크기 판정은 UTF-16 코드 단위가 아니라 바이트로 한다 — 한글이 과소 측정된다.
  function byteLength(body) {
    return new TextEncoder().encode(body).length;
  }

  // op 구현은 각자 파일에서 등록한다. 핸들러는 async (cmd, ctx) → {ok:false,…} | {ok:true,…}.
  var OPS = {};
  var ctx = {
    resolve: resolve, snapshot: snapshot, describe: describe,
    domHash: domHash, settle: settle, layout: function () { return layout; }
  };

  function registerOp(name, handler) { OPS[name] = handler; }

  async function runOp(handler, cmd) {
    try {
      return await handler(cmd, ctx);
    } catch (error) {
      return { ok: false, error: "js_exception", message: String((error && error.message) || error) };
    }
  }

  // snapshot: false를 준 op은 스냅샷을 원하지 않는다 — undefined는 JSON에서 통째로 사라진다.
  function resultFor(limits, done) {
    if (!done) return { ok: false, error: "unknown_op" };
    if (!done.ok) return done;
    return Object.assign({}, done, { snapshot: done.snapshot === false ? undefined : snapshot(limits) });
  }

  // attempt는 축소를 몇 번 거쳤는지다 — 0이면 원래 상한으로 한 번에 담겼다는 뜻이고,
  // 그 횟수를 스냅샷에 실어야 나중에 "상한이 실제로 좁았는가"를 기록에서 되물을 수 있다.
  function execOnce(limits, attempt, done) {
    var out;
    try {
      out = resultFor(limits, done);
    } catch (error) {
      out = { ok: false, error: "js_exception", message: String((error && error.message) || error) };
    }
    if (out.snapshot) {
      out.snapshot.shrinks = attempt;
      if (attempt > 0) out.snapshot.truncated = true;
    }
    return JSON.stringify(out);
  }

  // 상한 초과분은 보내기 전에 웹뷰에서 줄인다 — 512 KiB를 IPC로 보냈다 거절당하는 왕복을 없앤다.
  async function run(cmd) {
    var maxBytes = cmd.maxBytes || 512 * 1024;
    var limits = { maxDepth: cmd.maxDepth, maxNodes: cmd.maxNodes };
    // op은 루프 밖에서 한 번만 실행한다 — 축소 재시도가 다시 만드는 것은 뒤따르는 스냅샷뿐이다.
    var handler = OPS[cmd.op];
    var done = handler ? await runOp(handler, cmd) : null;
    for (var attempt = 0; attempt < 2; attempt++) {
      var body = execOnce(limits, attempt, done);
      if (byteLength(body) <= maxBytes) return body;
      limits = { maxDepth: Math.max(2, Math.floor((limits.maxDepth || 12) / 2)),
        maxNodes: Math.max(20, Math.floor((limits.maxNodes || 800) / 2)) };
    }
    return JSON.stringify({ ok: false, error: "payload_too_large" });
  }

  registerOp("snapshot", async function () { return { ok: true }; });

  window.__praxisPreviewExec = {
    snapshot: snapshot, resolve: resolve, run: run,
    _setLayout: setLayout, _registerOp: registerOp, _internal: ctx
  };
})();
