// ANSI → 구조화 세그먼트. 순수 로직. (설계 0013 §5.5)
//
// 모바일은 xterm.js를 쓰지 않는다. 읽기 전용이므로 커서 제어·리사이즈·WebGL이 전부
// 불필요하고, 그 셋이 모바일에서 가장 말썽인 부분이기도 하다. 여기서는 SGR(색·굵기)만
// 해석하고 나머지 제어 시퀀스는 버린다.

export interface AnsiStyle {
  fg?: string;
  bg?: string;
  bold?: boolean;
  dim?: boolean;
  underline?: boolean;
}

export interface AnsiSegment extends AnsiStyle {
  text: string;
}

export type AnsiLine = AnsiSegment[];

/** 표준 16색. 터미널은 다크 고정이므로(DESIGN.md) 대비를 그 전제로 고른다. */
const BASIC = [
  "#3f4451", // black — 순수 검정은 배경에 묻힌다
  "#e06c75",
  "#98c379",
  "#e5c07b",
  "#61afef",
  "#c678dd",
  "#56b6c2",
  "#abb2bf",
];
const BRIGHT = [
  "#5c6370",
  "#ff7b86",
  "#b5e890",
  "#ffd68a",
  "#7cc5ff",
  "#dd9bf0",
  "#6fd3de",
  "#ffffff",
];

/** xterm 256색 큐브 → hex. 16~231은 6x6x6 큐브, 232~255는 그레이스케일. */
function xterm256(index: number): string {
  if (index < 8) return BASIC[index];
  if (index < 16) return BRIGHT[index - 8];
  if (index < 232) {
    const n = index - 16;
    const level = (value: number) => (value === 0 ? 0 : 55 + value * 40);
    const r = level(Math.floor(n / 36) % 6);
    const g = level(Math.floor(n / 6) % 6);
    const b = level(n % 6);
    return rgb(r, g, b);
  }
  const gray = 8 + (index - 232) * 10;
  return rgb(gray, gray, gray);
}

function rgb(r: number, g: number, b: number): string {
  const hex = (value: number) => Math.max(0, Math.min(255, value)).toString(16).padStart(2, "0");
  return `#${hex(r)}${hex(g)}${hex(b)}`;
}

/** SGR 파라미터를 현재 스타일에 적용한다. 모르는 코드는 무시한다(추측해서 칠하지 않는다). */
function applySgr(style: AnsiStyle, params: number[]): AnsiStyle {
  let next: AnsiStyle = { ...style };
  for (let i = 0; i < params.length; i += 1) {
    const code = params[i];
    if (code === 0) next = {};
    else if (code === 1) next.bold = true;
    else if (code === 2) next.dim = true;
    else if (code === 4) next.underline = true;
    else if (code === 22) {
      next.bold = undefined;
      next.dim = undefined;
    } else if (code === 24) next.underline = undefined;
    else if (code >= 30 && code <= 37) next.fg = BASIC[code - 30];
    else if (code === 39) next.fg = undefined;
    else if (code >= 40 && code <= 47) next.bg = BASIC[code - 40];
    else if (code === 49) next.bg = undefined;
    else if (code >= 90 && code <= 97) next.fg = BRIGHT[code - 90];
    else if (code >= 100 && code <= 107) next.bg = BRIGHT[code - 100];
    else if (code === 38 || code === 48) {
      // 38;5;n (256색) 또는 38;2;r;g;b (트루컬러)
      const mode = params[i + 1];
      if (mode === 5 && params.length > i + 2) {
        const color = xterm256(params[i + 2]);
        if (code === 38) next.fg = color;
        else next.bg = color;
        i += 2;
      } else if (mode === 2 && params.length > i + 4) {
        const color = rgb(params[i + 2], params[i + 3], params[i + 4]);
        if (code === 38) next.fg = color;
        else next.bg = color;
        i += 4;
      } else {
        // 잘린 시퀀스 — 남은 파라미터를 색으로 오해하지 않도록 여기서 멈춘다.
        break;
      }
    }
  }
  return next;
}

const CSI = /\x1b\[([0-9;]*)([A-Za-z])/y;
const OSC = /\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/y;

/**
 * ANSI 텍스트를 줄 단위 세그먼트로 변환한다.
 *
 * - `\r`는 그 줄을 처음부터 다시 쓴다(진행률 표시가 남기는 잔상 제거).
 * - SGR 외의 CSI·OSC는 버린다. 커서 이동을 흉내 내려다 어긋난 화면을 그리는 것보다,
 *   출력 그대로 흐르는 로그로 보여주는 편이 읽기에 정확하다.
 */
export function parseAnsi(input: string): AnsiLine[] {
  const lines: AnsiLine[] = [];
  let current: AnsiLine = [];
  let style: AnsiStyle = {};
  let buffer = "";

  const flush = () => {
    if (buffer) {
      current.push({ text: buffer, ...style });
      buffer = "";
    }
  };
  const endLine = () => {
    flush();
    lines.push(current);
    current = [];
  };

  let index = 0;
  while (index < input.length) {
    const char = input[index];

    if (char === "\x1b") {
      CSI.lastIndex = index;
      const csi = CSI.exec(input);
      if (csi) {
        flush();
        if (csi[2] === "m") {
          const params = csi[1] === "" ? [0] : csi[1].split(";").map((p) => Number(p) || 0);
          style = applySgr(style, params);
        }
        index = CSI.lastIndex;
        continue;
      }
      OSC.lastIndex = index;
      const osc = OSC.exec(input);
      if (osc) {
        flush();
        index = OSC.lastIndex;
        continue;
      }
      // 알 수 없는 이스케이프 — 다음 문자까지 버린다.
      index += 2;
      continue;
    }

    if (char === "\n") {
      endLine();
      index += 1;
      continue;
    }
    if (char === "\r") {
      // 다음이 \n이면 CRLF 한 줄바꿈으로 본다.
      if (input[index + 1] === "\n") {
        endLine();
        index += 2;
        continue;
      }
      buffer = "";
      current = [];
      index += 1;
      continue;
    }
    // 나머지 C0 제어문자(벨·백스페이스 등)는 표시하지 않는다.
    if (char < " " && char !== "\t") {
      index += 1;
      continue;
    }

    buffer += char;
    index += 1;
  }

  flush();
  if (current.length > 0) lines.push(current);
  return lines;
}

/** 화면에 유지할 최대 줄 수. 초과분은 앞에서 버린다 — 폰 메모리와 렌더 비용의 상한. */
export const MAX_LINES = 1500;

export function capLines(lines: AnsiLine[], max = MAX_LINES): AnsiLine[] {
  return lines.length <= max ? lines : lines.slice(lines.length - max);
}
