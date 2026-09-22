import { useEffect, useRef, useState } from "react";
import { Icon } from "./icons";

/** 코드 블록 우상단 복사 버튼.
 *  가로 스크롤이 있는 블록은 드래그 선택으로 온전히 잡히지 않는다 — 원본 텍스트를 통째로 준다.
 *  스크롤 컨테이너 밖(블록 프레임)에 얹으므로 옆으로 밀어도 자리를 지킨다. */
export function CodeCopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  // 빈 블록에 손잡이를 달아 봐야 누르면 빈 클립보드가 된다.
  if (!text) return null;

  const copy = async () => {
    // clipboard가 없는 컨텍스트(비보안 오리진 등)에서 `?.`로 넘기면 undefined를 await 해
    // 실패가 성공으로 보인다 — 존재를 먼저 확인한다.
    const clipboard = navigator.clipboard;
    if (!clipboard) return;
    try {
      await clipboard.writeText(text);
    } catch {
      return; // 복사되지 않았으면 복사됐다고 말하지 않는다
    }
    setCopied(true);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), 1500);
  };

  const label = copied ? "복사됨" : "코드 복사";
  return (
    <button
      type="button"
      onClick={() => void copy()}
      aria-label={label}
      title={label}
      className="absolute right-1 top-1 z-10 rounded border border-transparent bg-bg/90 p-1
        text-text-muted opacity-50 transition
        hover:border-border hover:text-text hover:opacity-100 focus-visible:opacity-100"
    >
      <Icon name={copied ? "check" : "copy"} size={13} />
    </button>
  );
}
