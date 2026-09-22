import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import {
  agentActionKey,
  agentActionOpen,
  agentActionReplay,
  agentActionResize,
  agentActionWrite,
  type AgentActionKind,
} from "../../lib/ipc";
import { useXtermTheme } from "../TerminalView";
import { attachPtyStream } from "../../lib/pty-stream";
import { tryLoadWebgl, disposeWebgl, disposeTerminal } from "../../lib/xterm-webgl";

/**
 * 에이전트 CLI 액션 터미널 — 로그인·업데이트·자유 셸이 여기서 돈다.
 *
 * `ShellTerminal`과 같은 골격이지만 세션 키가 task id가 아니라 `"<kind>:<vendor>"`이고
 * 이벤트도 `action://`으로 분리돼 있다. 작업에 묶이지 않으므로 워크트리가 필요 없다.
 *
 * 로그인은 대화형이다 — device code 입력, 확인 프롬프트가 그대로 흐르므로 stdin을 연결한다.
 */
export function ActionTerminal({
  kind,
  vendor,
  onExit,
  codeFontFamily = "'JetBrains Mono','Fira Code',ui-monospace,monospace",
  codeFontSize = 13,
}: {
  kind: AgentActionKind;
  vendor: string;
  /** 프로세스가 끝났을 때 — 호출부가 상태를 다시 읽어 배지를 갱신한다. */
  onExit?: (code: number) => void;
  codeFontFamily?: string;
  codeFontSize?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const theme = useXtermTheme();
  const themeRef = useRef(theme);
  themeRef.current = theme;
  const onExitRef = useRef(onExit);
  onExitRef.current = onExit;

  useEffect(() => {
    const host = ref.current;
    if (!host) return;
    const key = agentActionKey(kind, vendor);
    let disposed = false;
    let opened = false;

    const term = new Terminal({
      fontFamily: codeFontFamily,
      fontSize: codeFontSize,
      cursorBlink: true,
      theme: themeRef.current,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    const webgl = tryLoadWebgl(term);

    const doFit = () => {
      if (disposed) return;
      try {
        fit.fit();
        if (opened) agentActionResize(key, term.cols, term.rows).catch(() => {});
      } catch {
        /* 아직 레이아웃 전 — 다음 RO에서 재시도 */
      }
    };

    let stopStream: (() => void) | undefined;
    // 첫 fit으로 실측 cols/rows를 얻은 뒤 연다(80x24 기본값으로 여는 것 방지).
    requestAnimationFrame(() => {
      if (disposed) return;
      try {
        fit.fit();
      } catch {
        /* 미레이아웃이면 기본 크기로 진행 */
      }
      term.focus();
      agentActionOpen(kind, vendor, term.cols, term.rows)
        .then((existed) => {
          if (disposed) return;
          opened = true;
          if (existed) agentActionResize(key, term.cols, term.rows).catch(() => {});
          return attachPtyStream({
            id: key,
            outputEvent: "action://output",
            exitEvent: "action://exit",
            fetchReplay: () => agentActionReplay(key),
            onData: (bytes) => {
              if (!disposed) term.write(bytes);
            },
            onExit: (code) => {
              if (disposed) return;
              opened = false;
              term.writeln(`\r\n\x1b[90m[종료: ${code}]\x1b[0m`);
              onExitRef.current?.(code);
            },
          });
        })
        .then((stop) => {
          if (!stop) return;
          if (disposed) stop();
          else stopStream = stop;
        })
        .catch((e) => {
          // 업데이트 거부(활성 작업 있음)도 여기로 온다 — 붉게 띄워 이유를 보여준다.
          if (!disposed) term.writeln(`\x1b[31m${String(e)}\x1b[0m`);
        });
    });

    const onData = term.onData((d) => {
      agentActionWrite(key, d).catch(() => {});
    });

    let rt: number | undefined;
    const ro = new ResizeObserver(() => {
      if (rt) clearTimeout(rt);
      rt = window.setTimeout(doFit, 60);
    });
    ro.observe(host);

    return () => {
      disposed = true;
      if (rt) clearTimeout(rt);
      ro.disconnect();
      onData.dispose();
      stopStream?.();
      disposeWebgl(webgl);
      disposeTerminal(term);
    };
    // 세션 키가 바뀌면 새 PTY다 — 폰트 변경만으로는 다시 열지 않는다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind, vendor]);

  return <div ref={ref} className="h-full w-full" />;
}
