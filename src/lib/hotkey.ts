/**
 * 핫키 표기 변환 — 브라우저 KeyboardEvent ↔ Tauri 단축키 문법 ↔ 사람이 읽는 표기.
 *
 * 저장값은 Tauri 문법 한 종류다(`Alt+Shift+C`). global-hotkey 파서가 그 문자열을 그대로
 * 먹기 때문에 중간 표현을 두지 않는다. 대신 **보여 줄 때만** 플랫폼 관례로 되돌린다 —
 * macOS 에 "Alt" 라는 이름의 키는 없다. 물리 키는 option(⌥) 이고 Tauri 가 그것을 Alt 로
 * 부를 뿐이라, 설정 화면이 "Alt" 라고 적으면 맥 사용자는 어느 키인지 알 수 없다.
 */

/** 조합의 주 키만 담는다 — modifier 는 별도 플래그로 다룬다. */
export interface Combo {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
  key: string;
}

/**
 * 키 캡처 결과. 실패를 한 종류로 뭉치지 않는 이유는 안내 문구가 서로 달라서다 —
 * "아직 누르는 중"과 "이 키는 쓸 수 없음"을 같은 말로 표시하면 사용자가 멈춘다.
 */
export type Capture =
  | { kind: "ok"; spec: string }
  /** modifier 만 눌린 중간 상태. 주 키를 기다린다. */
  | { kind: "pending" }
  /** 조합 없는 낱개 키 — 글로벌 핫키로 잡으면 그 키를 시스템 전역에서 빼앗는다. */
  | { kind: "bare" }
  /** global-hotkey 파서가 모르는 키(한/영, 국제 배열 전용 키 등). */
  | { kind: "unsupported" };

/**
 * 파서가 이름 그대로 받는 키들(`global-hotkey` 0.8 `parse_key`).
 * `KeyC`·`Digit1`·`F1`·`Numpad0` 은 규칙으로 잡히므로 여기 넣지 않는다.
 */
const NAMED_KEYS = new Set([
  "Backquote", "Backslash", "BracketLeft", "BracketRight", "Comma", "Equal", "Minus",
  "Period", "Quote", "Semicolon", "Slash",
  "Backspace", "CapsLock", "Delete", "End", "Enter", "Escape", "Home", "Insert",
  "NumLock", "PageDown", "PageUp", "Pause", "PrintScreen", "ScrollLock", "Space", "Tab",
  "ArrowDown", "ArrowLeft", "ArrowRight", "ArrowUp",
  "NumpadAdd", "NumpadDecimal", "NumpadDivide", "NumpadEnter", "NumpadEqual",
  "NumpadMultiply", "NumpadSubtract",
  "AudioVolumeDown", "AudioVolumeMute", "AudioVolumeUp",
  "MediaPlayPause", "MediaStop", "MediaTrackNext", "MediaTrackPrevious",
]);

const FUNCTION_KEY = /^F([1-9]|1\d|2[0-4])$/;
const NUMPAD_DIGIT = /^Numpad[0-9]$/;

/**
 * `KeyboardEvent.code` → Tauri 키 토큰.
 *
 * **`code` 를 쓰는 것이 이 모듈의 핵심이다.** macOS 에서 Option+C 를 누르면 `key` 는 `"ç"` 로
 * 온다 — 그대로 저장하면 파서가 거부하는 문자열이 된다. `code` 는 배열·modifier 와 무관하게
 * `"KeyC"` 로 오고, 파서가 그 이름을 알아본다.
 *
 * 알파벳·숫자만 짧은 형태로 줄인다. 파서는 `KeyC` 와 `C` 를 모두 받지만, 저장값은 사람이
 * 읽고 손으로 고칠 수도 있는 값이라 짧은 쪽을 정본으로 둔다.
 */
function keyToken(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];

  const digit = /^Digit(\d)$/.exec(code);
  if (digit) return digit[1];

  if (FUNCTION_KEY.test(code) || NUMPAD_DIGIT.test(code) || NAMED_KEYS.has(code)) return code;
  return null;
}

/** 낱개로 눌러도 되는 키 — 기능키는 어디서도 문자 입력에 쓰이지 않는다. */
const BARE_ALLOWED = (key: string) => FUNCTION_KEY.test(key);

/**
 * 눌린 키를 저장 가능한 단축키 문자열로 바꾼다.
 *
 * modifier 순서는 macOS 표기 순서(⌃⌥⇧⌘)로 고정한다. 파서는 순서를 가리지 않지만,
 * 저장값이 흔들리면 "같은 조합인데 문자열이 다른" 값이 DB 에 쌓여 비교가 번거로워진다.
 */
export function captureShortcut(
  event: Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">,
): Capture {
  const key = keyToken(event.code);
  if (key === null) {
    // modifier 자체를 누르는 중이면 아직 실패가 아니다 — 주 키가 뒤따라 온다.
    if (/^(Control|Alt|Shift|Meta)(Left|Right)$/.test(event.code)) return { kind: "pending" };
    return { kind: "unsupported" };
  }

  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Cmd");

  if (parts.length === 0 && !BARE_ALLOWED(key)) return { kind: "bare" };

  parts.push(key);
  return { kind: "ok", spec: parts.join("+") };
}

/**
 * 저장된 단축키 문자열을 분해한다. 파서와 같은 별칭을 받아들인다 —
 * `Option`=`Alt`, `Cmd`/`Command`/`Super`=meta, `CmdOrCtrl` 은 플랫폼에 따라 갈린다.
 */
export function parseShortcut(spec: string, mac: boolean): Combo | null {
  const combo: Combo = { ctrl: false, alt: false, shift: false, meta: false, key: "" };
  const tokens = spec.split("+").map((t) => t.trim()).filter((t) => t.length > 0);
  if (tokens.length === 0) return null;

  for (const token of tokens) {
    switch (token.toUpperCase()) {
      case "OPTION":
      case "ALT":
        combo.alt = true;
        break;
      case "CONTROL":
      case "CTRL":
        combo.ctrl = true;
        break;
      case "COMMAND":
      case "CMD":
      case "SUPER":
        combo.meta = true;
        break;
      case "SHIFT":
        combo.shift = true;
        break;
      case "COMMANDORCONTROL":
      case "COMMANDORCTRL":
      case "CMDORCTRL":
      case "CMDORCONTROL":
        if (mac) combo.meta = true;
        else combo.ctrl = true;
        break;
      default:
        // 주 키가 두 번 나오면 파서도 거부하는 형태다.
        if (combo.key) return null;
        combo.key = token;
    }
  }
  return combo.key ? combo : null;
}

/**
 * 비교용 정본 표기. 표기가 달라도 같은 조합이면 같은 문자열이 나온다 —
 * `Shift+Alt+C`·`Option+Shift+KeyC` → `Alt+Shift+C`.
 */
export function normalizeShortcut(spec: string, mac: boolean): string | null {
  const combo = parseShortcut(spec, mac);
  if (!combo) return null;

  const key = keyToken(combo.key) ?? keyToken(`Key${combo.key.toUpperCase()}`) ?? combo.key.toUpperCase();
  const parts: string[] = [];
  if (combo.ctrl) parts.push("Ctrl");
  if (combo.alt) parts.push("Alt");
  if (combo.shift) parts.push("Shift");
  if (combo.meta) parts.push("Cmd");
  parts.push(key);
  return parts.join("+");
}

/** 화살표는 어느 플랫폼에서도 기호가 짧고 분명하다. 나머지는 이름을 그대로 쓴다. */
const KEY_LABELS: Record<string, string> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Escape: "Esc",
};

/**
 * 사람이 읽는 표기. macOS 는 기호를 붙여 쓰고(⌥⇧C), 그 외에는 `+` 로 잇는다(Ctrl+Alt+C).
 * 해석할 수 없는 값은 원문 그대로 돌려준다 — 손으로 넣은 이상한 값을 감추면 고칠 수 없다.
 */
export function formatShortcut(spec: string, mac: boolean): string {
  const combo = parseShortcut(spec, mac);
  if (!combo) return spec;

  const key = KEY_LABELS[combo.key] ?? combo.key;
  if (mac) {
    return `${combo.ctrl ? "⌃" : ""}${combo.alt ? "⌥" : ""}${combo.shift ? "⇧" : ""}${combo.meta ? "⌘" : ""}${key}`;
  }
  const parts: string[] = [];
  if (combo.ctrl) parts.push("Ctrl");
  if (combo.alt) parts.push("Alt");
  if (combo.shift) parts.push("Shift");
  if (combo.meta) parts.push("Win");
  parts.push(key);
  return parts.join("+");
}

/**
 * 앱이 이미 쓰고 있어 음성 핫키가 가져갈 수 없는 조합.
 *
 * 지금은 비어 있다. 앱이 글로벌 핫키를 다시 잡으면 여기 적는다 —
 * 상수를 IPC 로 내보내지 않으므로 저쪽이 바뀌면 같이 고쳐야 한다.
 */
export const RESERVED_SHORTCUTS: { spec: string; owner: string }[] = [];

/**
 * 렌더러에서 플랫폼을 알 방법은 이것뿐이다 — OS 플러그인을 넣지 않았으므로 Tauri 가
 * 알려 주지 않는다. `navigator.platform` 은 deprecated 라 userAgent 를 본다.
 */
export function isMacPlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad/.test(navigator.userAgent);
}
