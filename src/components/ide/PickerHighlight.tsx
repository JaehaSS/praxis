import type { ReactNode } from "react";
import { matchRanges } from "./picker-search";

/** 매치 구간은 색이 아니라 웨이트로 밝힌다 — 새 액센트색을 만들지 않는다(Don't #5). */
export function Highlighted({ text, tokens }: { text: string; tokens: string[] }): ReactNode {
  const ranges = tokens.length > 0 ? matchRanges(text, tokens) : [];
  if (ranges.length === 0) return text;

  const parts: ReactNode[] = [];
  let at = 0;
  ranges.forEach(([start, end], i) => {
    if (start > at) parts.push(text.slice(at, start));
    parts.push(
      <span key={i} className="text-text font-medium">
        {text.slice(start, end)}
      </span>,
    );
    at = end;
  });
  if (at < text.length) parts.push(text.slice(at));
  return parts;
}
