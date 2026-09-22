import type { ReactNode } from "react";

/** react-markdown이 넘겨주는 <code> 엘리먼트의 얕은 형태. */
export interface CodeChild {
  props?: { className?: string; children?: ReactNode };
}

/** 코드 엘리먼트의 텍스트 콘텐츠를 추출(중첩 배열/문자열 대응) —
 *  클립보드에 넣을 원본, mermaid.render에 넘길 소스가 모두 이걸 쓴다.
 *  파서가 하나여야 복사한 것과 보이는 것이 어긋나지 않는다. */
export function extractText(node: ReactNode): string {
  if (typeof node === "string") return node;
  if (typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(extractText).join("");
  if (node && typeof node === "object" && "props" in node) {
    return extractText((node as CodeChild).props?.children);
  }
  return "";
}
