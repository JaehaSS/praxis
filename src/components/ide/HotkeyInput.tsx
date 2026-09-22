import { useState } from "react";
import { captureShortcut, formatShortcut, isMacPlatform, normalizeShortcut } from "../../lib/hotkey";

export interface HotkeyConflict {
  spec: string;
  owner: string;
}

interface Props {
  value: string;
  onChange: (spec: string) => void;
  /** 이 칸이 가져갈 수 없는 조합들. 겹치면 값을 넣지 않고 누가 쓰는지 알린다. */
  conflicts?: HotkeyConflict[];
}

/**
 * 단축키를 **눌러서** 넣는 칸.
 *
 * 자유 텍스트로 두면 사용자가 Tauri 문법을 알아야 하고, 틀린 문자열은 저장까지 간 뒤
 * 등록 단계에서야 실패한다. 여기서 잡으면 애초에 유효한 값만 나간다.
 */
export function HotkeyInput({ value, onChange, conflicts = [] }: Props) {
  const mac = isMacPlatform();
  const [capturing, setCapturing] = useState(false);
  const [preview, setPreview] = useState<string | null>(null);
  const [hint, setHint] = useState<string | null>(null);

  const ownerOf = (spec: string): string | null => {
    const target = normalizeShortcut(spec, mac);
    if (!target) return null;
    return conflicts.find((c) => normalizeShortcut(c.spec, mac) === target)?.owner ?? null;
  };

  const stopCapture = (element: HTMLElement) => {
    setPreview(null);
    element.blur();
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    // 캡처 중에는 앱 단축키가 먼저 먹지 않도록 전부 막는다 — ⌘W 로 창이 닫히면 안 된다.
    event.preventDefault();
    event.stopPropagation();
    const element = event.currentTarget;
    const bare = !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey;

    if (bare && event.code === "Escape") {
      setHint(null);
      stopCapture(element);
      return;
    }
    if (bare && (event.code === "Backspace" || event.code === "Delete")) {
      onChange("");
      setHint("핫키를 비웠습니다 — 이 모드는 꺼집니다");
      stopCapture(element);
      return;
    }

    const captured = captureShortcut(event);
    switch (captured.kind) {
      case "pending": {
        // 아직 주 키가 안 왔다. 지금까지 눌린 modifier 를 그대로 비춰 준다.
        const held = `${event.ctrlKey ? "⌃" : ""}${event.altKey ? "⌥" : ""}${event.shiftKey ? "⇧" : ""}${event.metaKey ? "⌘" : ""}`;
        setPreview(held ? `${held}…` : null);
        return;
      }
      case "bare":
        setHint("조합키와 함께 눌러 주세요 — 낱개 키는 전역에서 그 키를 빼앗습니다");
        return;
      case "unsupported":
        setHint("전역 단축키로 쓸 수 없는 키입니다");
        return;
      case "ok": {
        const owner = ownerOf(captured.spec);
        if (owner) {
          setHint(`이미 ${owner}이(가) 쓰는 조합입니다`);
          return;
        }
        onChange(captured.spec);
        setHint(null);
        stopCapture(element);
      }
    }
  };

  const shown = capturing
    ? (preview ?? "키를 누르세요")
    : value
      ? formatShortcut(value, mac)
      : "설정 안 됨";

  return (
    <>
      <input
        className={`w-full bg-bg border rounded px-2 py-1.5 text-sm outline-none ${
          capturing ? "border-primary text-text" : "border-border text-text"
        } ${value || capturing ? "" : "text-text-muted"}`}
        readOnly
        value={shown}
        spellCheck={false}
        onFocus={() => {
          setCapturing(true);
          setHint(null);
        }}
        onBlur={() => {
          setCapturing(false);
          setPreview(null);
        }}
        onKeyDown={onKeyDown}
      />
      {hint && <div className="text-status-failed text-[11px] mt-0.5">{hint}</div>}
    </>
  );
}
