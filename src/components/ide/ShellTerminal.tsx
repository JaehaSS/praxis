import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { shellOpen, shellWrite, shellResize, shellReplay, shellDetach } from "../../lib/ipc";
import { useXtermTheme } from "../TerminalView";
import { attachPtyStream, ptyEventNames } from "../../lib/pty-stream";
import { tryLoadWebgl, disposeWebgl, disposeTerminal } from "../../lib/xterm-webgl";

/** xterm 한 장이 붙는 백엔드 PTY 채널. 기본은 워크스페이스 셸(`shell://`)이고,
 *  에디터 팝아웃의 Python 콘솔은 같은 화면에 `repl://` 채널을 꽂는다 — 열기·쓰기·리사이즈·
 *  replay·detach의 계약이 같으므로 xterm 쪽 코드는 하나면 된다. */
export interface PtyChannel {
  /** 이벤트 접두어 — `ptyEventNames(prefix, id)`가 `<prefix>://output/<id>`를 만든다. */
  prefix: string;
  /** 세션 열기 — 이미 열려 있던 세션 재사용이면 true. 거부(throw)하면 화면에 사유를 적는다. */
  open: (id: number, cols: number, rows: number) => Promise<boolean>;
  write: (id: number, data: string) => Promise<unknown>;
  resize: (id: number, cols: number, rows: number) => Promise<unknown>;
  replay: (id: number) => Promise<string>;
  detach: (id: number) => Promise<unknown>;
}

/** 기본 채널. 각 항목을 함수로 감싸 **호출 시점에** ipc를 읽는다 — 직접 참조하면 이 모듈을
 *  import하는 것만으로 ipc의 shell* export가 있어야 해서, 셸과 무관한 화면의 테스트가
 *  `vi.mock("../../lib/ipc")`에 다섯 개를 채워 넣어야만 뜬다. */
export const SHELL_CHANNEL: PtyChannel = {
  prefix: "shell",
  open: (id, cols, rows) => shellOpen(id, cols, rows),
  write: (id, data) => shellWrite(id, data),
  resize: (id, cols, rows) => shellResize(id, cols, rows),
  replay: (id) => shellReplay(id),
  detach: (id) => shellDetach(id),
};

/** 도구 패널의 워크스페이스 셸 — 작업 워크트리에서 도는 인터랙티브 셸(로컬 전용).
 *  셸 프로세스는 패널을 닫아도 백엔드에 남는다(작업 종결/창 닫기 때 정리) —
 *  재진입 시 shell_open이 세션을 재사용하고, 백엔드 스크롤백(최대 2MiB)을 replay해
 *  이전 화면 내용을 그대로 복원한다. */
export function ShellTerminal({
  taskId,
  autoFocus = false,
  codeFontFamily = "'JetBrains Mono','Fira Code',ui-monospace,monospace",
  codeFontSize = 13,
  channel = SHELL_CHANNEL,
  onOpened,
}: {
  taskId: number;
  /** 마운트 직후 커서를 잡는다 — 단축키로 연 하단 도크는 바로 타이핑되어야 한다. */
  autoFocus?: boolean;
  codeFontFamily?: string;
  codeFontSize?: number;
  /** 붙을 PTY 채널. 기본은 워크스페이스 셸. 바꾸려면 `key`도 함께 바꿔 다시 마운트한다. */
  channel?: PtyChannel;
  /** 세션이 열려 스트림이 붙었다 — 열리기를 기다리던 입력(콘솔 실행 큐)을 흘려보낼 시점. */
  onOpened?: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const theme = useXtermTheme();
  const themeRef = useRef(theme);
  themeRef.current = theme;
  const autoFocusRef = useRef(autoFocus);
  autoFocusRef.current = autoFocus;
  const channelRef = useRef(channel);
  channelRef.current = channel;
  const onOpenedRef = useRef(onOpened);
  onOpenedRef.current = onOpened;

  useEffect(() => {
    const host = ref.current;
    if (!host) return;
    let disposed = false;
    let opened = false;
    const ch = channelRef.current;

    const term = new Terminal({
      fontFamily: codeFontFamily,
      fontSize: codeFontSize,
      cursorBlink: true,
      theme: themeRef.current,
    });
    termRef.current = term;
    const fit = new FitAddon();
    fitRef.current = fit;
    term.loadAddon(fit);
    term.open(host);
    const webgl = tryLoadWebgl(term);

    const doFit = () => {
      if (disposed) return;
      try {
        fit.fit();
        if (opened) ch.resize(taskId, term.cols, term.rows).catch(() => {});
      } catch {
        /* 아직 레이아웃 전 — 다음 RO에서 재시도 */
      }
    };

    let stopStream: (() => void) | undefined;
    // 첫 fit으로 실측 cols/rows를 얻은 뒤 셸을 연다(80x24 기본값으로 여는 것 방지).
    requestAnimationFrame(() => {
      if (disposed) return;
      try {
        fit.fit();
      } catch {
        /* 미레이아웃이면 기본 크기로 진행 */
      }
      if (autoFocusRef.current) term.focus();
      ch.open(taskId, term.cols, term.rows)
        .then((existed) => {
          if (disposed) {
            // open이 끝나기 전에 화면이 사라졌다 — attach만 남으면 이 셸은 회수되지 않는다.
            ch.detach(taskId).catch(() => {});
            return;
          }
          opened = true;
          if (existed) ch.resize(taskId, term.cols, term.rows).catch(() => {});
          // replay(스크롤백)를 선전송한 뒤 라이브 스트림을 구독한다 — 재사용 세션이면
          // 실제 이전 화면 내용이, 새 세션이면 빈 replay가 온다(Ctrl-L 프롬프트 재표시 대체).
          return attachPtyStream({
            id: taskId,
            ...ptyEventNames(ch.prefix, taskId),
            fetchReplay: () => ch.replay(taskId),
            onData: (bytes) => {
              if (!disposed) term.write(bytes);
            },
            onExit: (code) => {
              if (!disposed) {
                opened = false;
                term.writeln(`\r\n\x1b[90m[${ch.prefix} exited: ${code}]\x1b[0m`);
              }
            },
          });
        })
        .then((stop) => {
          if (!stop) return;
          if (disposed) {
            stop();
            return;
          }
          stopStream = stop;
          onOpenedRef.current?.();
        })
        .catch((e) => {
          if (!disposed) term.writeln(`\x1b[31m${String(e)}\x1b[0m`);
        });
    });

    const onData = term.onData((d) => {
      ch.write(taskId, d).catch(() => {});
    });

    let rt: number | undefined;
    const ro = new ResizeObserver(() => {
      if (rt) clearTimeout(rt);
      rt = window.setTimeout(doFit, 60);
    });
    ro.observe(host);

    return () => {
      disposed = true;
      // 셸 프로세스는 그대로 둔다(스크롤백을 지키려면 그래야 한다) — 보는 화면만 뗀다.
      if (opened) ch.detach(taskId).catch(() => {});
      if (rt) clearTimeout(rt);
      ro.disconnect();
      onData.dispose();
      stopStream?.();
      disposeWebgl(webgl);
      disposeTerminal(term);
      termRef.current = null;
      fitRef.current = null;
    };
  }, [taskId]);

  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = theme;
  }, [theme]);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.fontFamily = codeFontFamily;
    term.options.fontSize = codeFontSize;
    try {
      fitRef.current?.fit();
      channelRef.current.resize(taskId, term.cols, term.rows).catch(() => {});
    } catch {
      /* 아직 레이아웃 전 — 다음 리사이즈에서 재계산 */
    }
  }, [codeFontFamily, codeFontSize, taskId]);

  return <div ref={ref} className="flex-1 min-h-0 bg-bg" />;
}
