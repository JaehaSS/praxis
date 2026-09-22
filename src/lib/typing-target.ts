/**
 * 전역 단축키가 입력을 가로채면 안 되는 자리인지 — Monaco와 xterm도 textarea를 쓰므로
 * 에디터·터미널·컴포저가 모두 여기에 걸린다. Delete(지우기)와 ⌘Z(되돌리기) 양쪽이 쓴다.
 */
export const isTypingTarget = (): boolean => {
  const element = document.activeElement;
  if (!(element instanceof HTMLElement)) return false;
  return (
    element.isContentEditable
    || element.tagName === "INPUT"
    || element.tagName === "TEXTAREA"
    || element.tagName === "SELECT"
  );
};
