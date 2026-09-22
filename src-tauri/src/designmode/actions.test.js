// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, it, expect, beforeEach } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
const load = (name) => readFileSync(path.join(here, name), "utf-8");
const [execSource, actionsSource] = [load("exec.js"), load("actions.js")];
beforeEach(() => {
  document.body.innerHTML = "";
  new Function(execSource)();
  new Function(actionsSource)();
});

const exec = () => window.__praxisPreviewExec;
const refOf = (name) => exec().snapshot().text.match(new RegExp(`"${name}"[^\\n]*\\[ref=(s\\d+e\\d+)\\]`))[1];
const seeAs = (hit) => exec()._setLayout({
  rects: () => [{}],
  rect: () => ({ left: 0, top: 0, width: 20, height: 10 }),
  elementAt: () => hit
});

describe("actions", () => {
  it("clicks a button, reports changed and returns a fresh snapshot", async () => {
    document.body.innerHTML = `<button>로그인</button>`;
    const button = document.querySelector("button");
    button.addEventListener("click", () => button.insertAdjacentHTML("afterend", "<p>환영</p>"));
    const ref = refOf("로그인");
    seeAs(button);
    const res = JSON.parse(await exec().run({ op: "click", ref }));
    expect(res).toMatchObject({ ok: true, changed: true, target: 'button "로그인"' });
    expect(res.snapshot.generation).toBeGreaterThan(1);
  });
  it("reports changed=false when the click leaves the DOM untouched", async () => {
    document.body.innerHTML = `<button>확인</button>`;
    const button = document.querySelector("button");
    const ref = refOf("확인");
    seeAs(button);
    const res = JSON.parse(await exec().run({ op: "click", ref }));
    expect(res.ok).toBe(true);
    expect(res.changed).toBe(false);
  });
  it("dispatches the full pointer sequence before the click", async () => {
    document.body.innerHTML = `<button>순서</button>`;
    const button = document.querySelector("button");
    const seen = [];
    for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]) {
      button.addEventListener(type, (e) => { seen.push(type); expect(e.bubbles).toBe(true); });
    }
    const ref = refOf("순서");
    seeAs(button);
    await exec().run({ op: "click", ref });
    expect(seen).toEqual(["pointerdown", "mousedown", "pointerup", "mouseup", "click"]);
  });
  it("rejects a ref from an earlier generation", async () => {
    document.body.innerHTML = `<button>확인</button>`;
    const ref = refOf("확인");
    exec().snapshot();
    expect(JSON.parse(await exec().run({ op: "click", ref }))).toEqual({ ok: false, error: "stale_ref" });
  });
  it("rejects an element with no client rects as not_visible", async () => {
    document.body.innerHTML = `<button>숨은</button>`;
    const ref = refOf("숨은");
    exec()._setLayout({ rects: () => [] });
    expect(JSON.parse(await exec().run({ op: "click", ref }))).toEqual({ ok: false, error: "not_visible" });
  });
  it("rejects an offscreen element whose center hits nothing", async () => {
    document.body.innerHTML = `<button>바깥</button>`;
    const ref = refOf("바깥");
    seeAs(null);
    expect(JSON.parse(await exec().run({ op: "click", ref }))).toEqual({ ok: false, error: "not_visible" });
  });
  it("rejects a disabled button", async () => {
    document.body.innerHTML = `<button disabled>보내기</button>`;
    const ref = refOf("보내기");
    seeAs(document.querySelector("button"));
    expect(JSON.parse(await exec().run({ op: "click", ref }))).toEqual({ ok: false, error: "disabled" });
  });
  it("rejects an aria-disabled element and one inside a disabled fieldset", async () => {
    document.body.innerHTML = `<button aria-disabled="true">가짜</button><fieldset disabled><button>안쪽</button></fieldset>`;
    exec().snapshot();
    for (const name of ["가짜", "안쪽"]) {
      const ref = refOf(name);
      seeAs(document.body.querySelector("button"));
      expect(JSON.parse(await exec().run({ op: "click", ref })).error).toBe("disabled");
    }
  });
  it("reports the covering element when the click point hits something else", async () => {
    document.body.innerHTML = `<button>로그인</button><div>모달</div>`;
    const ref = refOf("로그인");
    seeAs(document.querySelector("div"));
    expect(JSON.parse(await exec().run({ op: "click", ref })))
      .toEqual({ ok: false, error: "obscured", obscured_by: 'div "모달"' });
  });
  it("fills an input through the native setter and fires input and change", async () => {
    document.body.innerHTML = `<input aria-label="이메일">`;
    const input = document.querySelector("input");
    const seen = [];
    input.addEventListener("input", (e) => seen.push(["input", e.target.value]));
    input.addEventListener("change", (e) => seen.push(["change", e.target.value]));
    const ref = refOf("이메일");
    seeAs(input);
    const res = JSON.parse(await exec().run({ op: "fill", ref, text: "a@b.c" }));
    expect(res).toMatchObject({ ok: true, target: 'textbox "이메일"' });
    expect(input.value).toBe("a@b.c");
    expect(seen).toEqual([["input", "a@b.c"], ["change", "a@b.c"]]);
  });
  it("never echoes the filled secret back in the result", async () => {
    document.body.innerHTML = `<input type="password" aria-label="비밀번호">`;
    const input = document.querySelector("input");
    const ref = refOf("비밀번호");
    seeAs(input);
    const body = await exec().run({ op: "fill", ref, text: "hunter2" });
    expect(input.value).toBe("hunter2");
    expect(body).not.toContain("hunter2");
  });
  it("refuses to fill a non-editable element", async () => {
    document.body.innerHTML = `<div role="button">상자</div>`;
    const ref = refOf("상자");
    seeAs(document.querySelector("div"));
    expect(JSON.parse(await exec().run({ op: "fill", ref, text: "x" })))
      .toEqual({ ok: false, error: "not_fillable" });
  });
  it("submits the surrounding form when Enter is not prevented", async () => {
    document.body.innerHTML = `<form><input aria-label="이메일"></form>`;
    const input = document.querySelector("input");
    let submits = 0;
    document.querySelector("form").requestSubmit = () => { submits += 1; };
    const ref = refOf("이메일");
    seeAs(input);
    const res = JSON.parse(await exec().run({ op: "press_key", key: "Enter", ref }));
    expect(res.ok).toBe(true);
    expect(submits).toBe(1);
  });
  it("does not submit when a keydown listener prevents Enter", async () => {
    document.body.innerHTML = `<form><input aria-label="이메일"></form>`;
    const input = document.querySelector("input");
    let submits = 0;
    document.querySelector("form").requestSubmit = () => { submits += 1; };
    input.addEventListener("keydown", (e) => e.preventDefault());
    const ref = refOf("이메일");
    seeAs(input);
    await exec().run({ op: "press_key", key: "Enter", ref });
    expect(submits).toBe(0);
  });
  it("sends a key without a ref to the focused element", async () => {
    document.body.innerHTML = `<input aria-label="검색">`;
    const input = document.querySelector("input");
    input.focus();
    const seen = [];
    for (const type of ["keydown", "keypress", "keyup"]) input.addEventListener(type, (e) => seen.push([type, e.key]));
    const res = JSON.parse(await exec().run({ op: "press_key", key: "a" }));
    expect(res).toMatchObject({ ok: true, target: 'textbox "검색"' });
    expect(seen).toEqual([["keydown", "a"], ["keypress", "a"], ["keyup", "a"]]);
  });
  it("omits keypress for non-printable keys", async () => {
    document.body.innerHTML = `<input aria-label="검색">`;
    const input = document.querySelector("input");
    input.focus();
    const seen = [];
    for (const type of ["keydown", "keypress", "keyup"]) input.addEventListener(type, (e) => seen.push(type));
    await exec().run({ op: "press_key", key: "Tab" });
    expect(seen).toEqual(["keydown", "keyup"]);
  });
  it("reports js_exception with a message when the layout probe throws", async () => {
    document.body.innerHTML = `<button>확인</button>`;
    const ref = refOf("확인");
    exec()._setLayout({ rects: () => [{}], rect: () => { throw new Error("boom"); } });
    expect(JSON.parse(await exec().run({ op: "click", ref })))
      .toEqual({ ok: false, error: "js_exception", message: "boom" });
  });
  it("leaves no highlight box in the snapshot text", async () => {
    document.body.innerHTML = `<button>확인</button>`;
    const button = document.querySelector("button");
    const ref = refOf("확인");
    seeAs(button);
    const res = JSON.parse(await exec().run({ op: "click", ref }));
    expect(res.snapshot.text).not.toContain("praxis-highlight");
    expect(document.querySelectorAll("[data-praxis-highlight]").length).toBe(1);
  });
});

describe("preview exec — action robustness", () => {
  beforeEach(() => {
    document.body.innerHTML = '<button id="b">Go</button>';
    const target = document.getElementById("b");
    window.__praxisPreviewExec._setLayout({
      rects: () => [{}],
      rect: () => ({ left: 0, top: 0, width: 10, height: 10 }),
      elementAt: () => target,
    });
  });

  it("does not report a change when only the highlight box disappears mid-settle", async () => {
    const snap = window.__praxisPreviewExec.snapshot();
    const ref = snap.text.match(/ref=(s\d+e\d+)/)[1];
    const raf = window.requestAnimationFrame;
    // 느린 프레임: 박스의 600ms 타이머가 settle보다 먼저 끝난다.
    window.requestAnimationFrame = (cb) => setTimeout(cb, 400);
    try {
      const res = JSON.parse(await window.__praxisPreviewExec.run({ op: "click", ref }));
      expect(res.ok).toBe(true);
      expect(res.changed).toBe(false);
    } finally {
      window.requestAnimationFrame = raf;
    }
  }, 5000);

  it("settles even when requestAnimationFrame never fires", async () => {
    const snap = window.__praxisPreviewExec.snapshot();
    const ref = snap.text.match(/ref=(s\d+e\d+)/)[1];
    const raf = window.requestAnimationFrame;
    window.requestAnimationFrame = () => 0;
    try {
      const res = JSON.parse(await window.__praxisPreviewExec.run({ op: "click", ref }));
      expect(res.ok).toBe(true);
    } finally {
      window.requestAnimationFrame = raf;
    }
  }, 3000);
});

describe("wait_for", () => {
  const run = (cmd) => window.__praxisPreviewExec.run(cmd).then(JSON.parse);

  it("satisfies immediately when the text is already on the page", async () => {
    document.body.innerHTML = `<p>환영합니다</p>`;
    const res = await run({ op: "wait_for", text: "환영", timeout_ms: 5000 });
    expect(res.ok).toBe(true);
    expect(res.satisfied).toBe(true);
    expect(res.elapsed_ms).toBeLessThan(100);
    expect(res.snapshot.url).toBeDefined();
  });

  it("polls until the text appears", async () => {
    document.body.innerHTML = `<p>불러오는 중</p>`;
    setTimeout(() => { document.body.innerHTML = `<p>완료</p>`; }, 150);
    const res = await run({ op: "wait_for", text: "완료", timeout_ms: 3000 });
    expect(res.satisfied).toBe(true);
    expect(res.elapsed_ms).toBeGreaterThanOrEqual(100);
  }, 5000);

  it("waits for text to disappear with gone", async () => {
    document.body.innerHTML = `<p>스피너</p>`;
    setTimeout(() => { document.body.innerHTML = `<p>끝</p>`; }, 120);
    const res = await run({ op: "wait_for", gone: "스피너", timeout_ms: 3000 });
    expect(res.satisfied).toBe(true);
  }, 5000);

  it("reports satisfied=false once the timeout elapses", async () => {
    document.body.innerHTML = `<p>가만히</p>`;
    const res = await run({ op: "wait_for", text: "오지 않는다", timeout_ms: 200 });
    expect(res.ok).toBe(true);
    expect(res.satisfied).toBe(false);
    expect(res.elapsed_ms).toBeGreaterThanOrEqual(200);
  }, 5000);

  it("requires text or gone", async () => {
    expect(await run({ op: "wait_for", timeout_ms: 100 })).toEqual({ ok: false, error: "invalid_argument" });
  });

  it("needs both conditions to hold at once", async () => {
    document.body.innerHTML = `<p>완료 스피너</p>`;
    const res = await run({ op: "wait_for", text: "완료", gone: "스피너", timeout_ms: 150 });
    expect(res.satisfied).toBe(false);
  }, 5000);
});
