import { memo, useEffect, useRef } from "react";

/** 파일 뷰어용 HTML 렌더러 — 원문을 샌드박스 iframe에 그대로 얹는다.
 *
 *  `sandbox=""`(빈 값)이 이 기능의 유일한 안전 장치다. 워크트리의 HTML은 에이전트가 만들었든
 *  받아왔든 신뢰할 수 없는 입력이므로 `allow-scripts`·`allow-same-origin`을 **절대 넣지 않는다** —
 *  빈 값이면 스크립트 실행·폼 제출·상위 프레임 내비게이션·팝업이 모두 막히고 불투명 출처가 되어
 *  앱의 DOM·스토리지에 닿을 수 없다.
 *
 *  srcdoc이라 문서에 기준 URL이 없다: `./style.css`나 `<img src="a.png">` 같은 상대 경로 자원은
 *  해석되지 않는다. 브라우저에서 열었을 때와 달리 이런 자원은 빠진 채로 보인다.
 *
 *  bg-white: 문서가 자기 배경을 지정하지 않으면 앱의 어두운 배경이 비쳐 검은 글자가 안 보인다.
 *  브라우저와 같이 흰 페이지 위에 얹는다.
 *
 *  className: 호출자가 크기를 정한다. 빈 sandbox의 불투명 출처라 부모가 `contentDocument`를 읽을
 *  수 없고, 따라서 **내용에 맞춘 자동 높이 조절이 불가능하다** — 전면 표시가 아닌 자리(채팅 버블
 *  등)는 고정 높이를 주고 안쪽 스크롤에 맡겨야 한다.
 *
 *  memo: html 동일 시 iframe 재로드 방지. */
export const HtmlDoc = memo(function HtmlDoc({
  html,
  title,
  retainKeyboardFocus = false,
  className = "w-full h-full border-0 bg-white",
}: { html: string; title: string; className?: string; retainKeyboardFocus?: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  useEffect(() => {
    if (!retainKeyboardFocus) return;
    let pending: ReturnType<typeof setTimeout> | undefined;
    const restore = () => {
      clearTimeout(pending);
      // iframe 키 이벤트는 부모로 버블링하지 않는다. 불투명 출처를 유지한 채
      // 미리보기의 키보드 포커스만 부모로 되돌린다.
      pending = setTimeout(() => {
        if (document.activeElement === frame.current) host.current?.focus({ preventScroll: true });
      }, 0);
    };
    window.addEventListener("blur", restore);
    frame.current?.addEventListener("focus", restore);
    const element = frame.current;
    return () => {
      clearTimeout(pending);
      window.removeEventListener("blur", restore);
      element?.removeEventListener("focus", restore);
    };
  }, [retainKeyboardFocus]);
  const preview = <iframe ref={frame} srcDoc={html} sandbox="" title={title} className={className} />;
  return retainKeyboardFocus
    ? <div ref={host} tabIndex={-1} className="w-full h-full outline-none">{preview}</div>
    : preview;
});
