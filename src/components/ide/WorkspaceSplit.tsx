import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactElement,
  type ReactNode,
} from "react";
import {
  clampSplit,
  nextSplitMode,
  readSplit,
  writeSplit,
  type SplitMode,
} from "./workspace-split-width";

interface Props {
  /** 왼쪽 열 — 대화(대화 모드) 또는 출력 로그(에이전트 모드). */
  session: ReactNode;
  /** 오른쪽 열 — 파일/Diff/프리뷰 탭. */
  code: ReactNode;
  /**
   * 코드 열이 자리를 받는지. 소유자는 세션 헤더의 코드 버튼이다 — 여기서 상태를 갖지 않는
   * 이유는 헤더와 이 셸이 같은 하나를 움직여야 하기 때문이다(ADR 0111).
   */
  codeOpen: boolean;
  /** 경계 더블클릭으로 코드 열을 닫을 때. 헤더 버튼을 다시 누른 것과 같다. */
  onCloseCode: () => void;
  /**
   * 중앙에 남은 폭이 바뀔 때. 여기서 이미 재고 있는 값이라 다시 관측하지 않는다 —
   * 플로팅 채널이 뜰 자리가 있는지를 App이 이 값으로 판정한다.
   */
  onAvailableWidth?: (width: number) => void;
  onModeChange?: (mode: SplitMode) => void;
}

const readWidth = () => (typeof localStorage === "undefined" ? 480 : readSplit(localStorage).width);

const persist = (width: number) => {
  if (typeof localStorage !== "undefined") writeSplit(localStorage, { width });
};

/** 파일 트리(208) + 좌우 여백을 넉넉히 뺀 창 폭 — 첫 프레임 추정용. */
const CHROME_ALLOWANCE = 240;

/**
 * ResizeObserver의 첫 콜백이 오기 전 한 프레임의 모드.
 *
 * 창 폭으로 추정한다 — 실제 중앙 폭은 이보다 좁으므로 크롬 몫을 빼서 보수적으로 잡는다.
 * 추정을 생략하면 좁은 창을 열 때 2열이 한 프레임 그려졌다가 접히는 것이 눈에 띈다.
 */
const initialMode = (): SplitMode => {
  if (typeof window === "undefined") return "split";
  return nextSplitMode("tabs", window.innerWidth - CHROME_ALLOWANCE);
};

/**
 * 워크스페이스 중앙의 `세션 | 코드` 2열 셸.
 *
 * 열려 있는 동안 탭이 아니라 열인 이유는 "한 번에 하나만 본다"는 전제가 틀렸기 때문이다 —
 * 코드를 보는 동안에도 대화가 이어진다(설계 0045). 폭이 모자라면 탭으로 폴백하되, 두 임계
 * 사이에서는 직전 모드를 유지해 경계에서 레이아웃이 진동하지 않게 한다.
 *
 * 다만 **기본은 세션 한 열**이다. 코드 열은 헤더 버튼이 부를 때만 자리를 받는다(ADR 0111) —
 * ADR 0108이 상시 2열로 두었던 것을 뒤집은 부분이다. 여기서 판단하지 않고 `codeOpen`을 받는
 * 이유는, 헤더 버튼과 경계 더블클릭이 같은 하나를 움직여야 하기 때문이다.
 */
export function WorkspaceSplit({
  session,
  code,
  codeOpen,
  onCloseCode,
  onAvailableWidth,
  onModeChange,
}: Props): ReactElement {
  const hostRef = useRef<HTMLDivElement>(null);
  const [sessionWidth, setSessionWidth] = useState(readWidth);
  const [available, setAvailable] = useState<number | null>(null);
  const [mode, setMode] = useState<SplitMode>(initialMode);
  useLayoutEffect(() => { onModeChange?.(mode); }, [mode, onModeChange]);

  // 저장 폭은 드래그하던 해상도에서만 검증된 값이다 — 표시할 때마다 현재 가용 폭에 다시
  // 가둔다. 상태를 덮어쓰지 않는 이유는 저장값이 "의도한 폭"이라서다: 창이 좁아진 동안만
  // 물러났다가, 폭이 돌아오면 원래 폭이 복원돼야 한다.
  const effectiveWidth = available == null ? sessionWidth : clampSplit(sessionWidth, available);

  // 창·트리·패널 어느 쪽이 바뀌든 결과는 "중앙에 남은 폭" 하나로 수렴한다 — 그 값만 본다.
  useEffect(() => {
    const host = hostRef.current;
    if (host == null || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width;
      if (typeof width === "number") setAvailable(width);
    });
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (available == null) return;
    setMode((current) => nextSplitMode(current, available));
    onAvailableWidth?.(available);
  }, [available, onAvailableWidth]);

  const resizeTo = (next: number) => {
    if (available == null) return;
    const width = clampSplit(next, available);
    setSessionWidth(width);
    return width;
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = effectiveWidth;
    let finalWidth = effectiveWidth;
    const move = (moveEvent: PointerEvent) => {
      const next = resizeTo(startWidth + moveEvent.clientX - startX);
      if (next != null) finalWidth = next;
    };
    const stop = () => {
      persist(finalWidth);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = resizeTo(effectiveWidth + (event.key === "ArrowLeft" ? -24 : 24));
    if (next != null) persist(next);
  };

  // 좁은 창에서는 두 열을 모두 마운트한 채 보이는 쪽만 바꾼다. 대화의 스크롤 ref와
  // 컴포저의 로컬 초안은 이 전환으로 사라지면 안 된다.
  if (mode === "tabs") {
    return (
      <div ref={hostRef} className="flex-1 flex min-h-0 min-w-0 flex-col">
        <div className={`${codeOpen ? "hidden" : "flex"} min-h-0 flex-1 flex-col`} inert={codeOpen}>{session}</div>
        <div className={`${codeOpen ? "flex" : "hidden"} min-h-0 flex-1 flex-col`} inert={!codeOpen}>{code}</div>
      </div>
    );
  }

  return (
    <div ref={hostRef} className="flex-1 flex min-h-0 min-w-0">
      <div
        className="flex min-h-0 flex-col"
        style={codeOpen ? { width: effectiveWidth, flexShrink: 0 } : { flex: 1, minWidth: 0 }}
      >
        {session}
      </div>
      {codeOpen && (
        <>
          <div
            role="separator"
            aria-label="세션·코드 열 너비 조절"
            aria-orientation="vertical"
            tabIndex={0}
            className="relative z-10 w-1 shrink-0 cursor-col-resize bg-border hover:bg-primary/50 focus:bg-primary/50"
            onPointerDown={onPointerDown}
            onDoubleClick={onCloseCode}
            onKeyDown={onKeyDown}
          />
          <div key="code" className="flex min-h-0 min-w-0 flex-1 flex-col">{code}</div>
        </>
      )}
    </div>
  );
}
