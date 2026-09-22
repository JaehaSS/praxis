/** 터미널 출력(PTY 원본)의 평문 파일 경로를 xterm 링크로 만든다.
 *  경로 판정은 Markdown 쪽과 같은 규칙을 쓴다(file-path-links.ts) — 두 뷰가 같은 것을 링크한다.
 *
 *  xterm 좌표는 두 군데서 어긋나기 쉽다.
 *  ① **셀 ≠ 문자**: 한글은 2셀을 차지하므로 문자열 인덱스를 그대로 x로 쓰면 밀린다. 셀을 훑으며
 *     문자마다 좌표를 기록해 매핑을 만든다(width 0인 셀은 wide 문자의 뒤쪽 절반이라 건너뛴다).
 *  ② **줄바꿈**: 긴 경로는 wrap되어 두 줄에 걸친다. isWrapped를 따라 논리 줄을 복원해야 잘리지
 *     않는다 — 그래서 링크 range의 start.y와 end.y가 다를 수 있다(xterm이 허용하는 형태).
 */

import { findFilePathSpans } from "./file-path-links";

/** xterm IBufferCell에서 실제로 쓰는 부분만. */
export interface CellLike {
  getChars(): string;
  getWidth(): number;
}

/** xterm IBufferLine에서 실제로 쓰는 부분만. */
export interface BufferLineLike {
  readonly length: number;
  readonly isWrapped: boolean;
  getCell(x: number): CellLike | undefined;
}

/** xterm IBuffer에서 실제로 쓰는 부분만. */
export interface BufferLike {
  readonly length: number;
  getLine(y: number): BufferLineLike | undefined;
}

export interface TerminalLink {
  /** 클릭 시 열 대상 — 경로 원문(라인 접미 포함) */
  text: string;
  /** xterm ILink.range와 같은 형태(1-based, 양끝 포함) */
  range: {
    start: { x: number; y: number };
    end: { x: number; y: number };
  };
}

interface CellCoord {
  /** 0-based 셀 x */
  x: number;
  /** 0-based 버퍼 라인 y */
  y: number;
  /** 셀 폭(1 또는 2) */
  width: number;
}

/** wrap을 이어붙여 논리 줄 하나를 복원한다. coords[i]는 text[i]가 놓인 셀. */
export function logicalLineAt(
  buffer: BufferLike,
  lineIndex: number,
): { text: string; coords: CellCoord[] } {
  let start = lineIndex;
  while (start > 0 && buffer.getLine(start)?.isWrapped) start--;

  let text = "";
  const coords: CellCoord[] = [];
  for (let y = start; y < buffer.length; y++) {
    const line = buffer.getLine(y);
    if (!line) break;
    if (y > start && !line.isWrapped) break;
    for (let x = 0; x < line.length; x++) {
      const cell = line.getCell(x);
      if (!cell) continue;
      const width = cell.getWidth();
      if (width === 0) continue;
      const chars = cell.getChars() || " ";
      for (let i = 0; i < chars.length; i++) coords.push({ x, y, width });
      text += chars;
    }
  }
  return { text, coords };
}

/** xterm ILinkProvider.provideLinks의 y(1-based 버퍼 라인)에 대한 파일 경로 링크. */
export function findTerminalFileLinks(buffer: BufferLike, bufferLineNumber: number): TerminalLink[] {
  const { text, coords } = logicalLineAt(buffer, bufferLineNumber - 1);
  const links: TerminalLink[] = [];
  for (const span of findFilePathSpans(text)) {
    const first = coords[span.start];
    const last = coords[span.end - 1];
    if (!first || !last) continue;
    links.push({
      text: span.text,
      range: {
        start: { x: first.x + 1, y: first.y + 1 },
        end: { x: last.x + last.width, y: last.y + 1 },
      },
    });
  }
  return links;
}
