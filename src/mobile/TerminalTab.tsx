import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import { capLines, parseAnsi, type AnsiLine } from "./ansi";
import { Empty, Spinner } from "./primitives";

// 읽기 전용 터미널 — replay + live. (설계 0013 §5.5)
// 입력은 받지 않는다(대화 탭 담당). 스크롤이 바닥에 있을 때만 따라 내려간다 —
// 위를 읽는 중에 새 출력이 화면을 끌고 가면 로그를 읽을 수 없다.

const BOTTOM_SLACK_PX = 48;

export function TerminalTab({ id }: { id: number }) {
  const [lines, setLines] = useState<AnsiLine[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const stickToBottom = useRef(true);

  useEffect(() => {
    let cancelled = false;
    // 원문을 누적해 두고 매번 통째로 파싱한다. 청크 경계가 이스케이프 시퀀스 한가운데를
    // 가를 수 있어, 조각별로 파싱하면 색이 깨진다.
    let raw = "";
    let after = 0;
    setLines(null);
    setError(null);

    let chain = Promise.resolve();
    const load = () => {
      chain = chain.then(async () => {
        try {
          for (;;) {
            if (cancelled) return;
            const rows = await api.taskOutput(id, after);
            if (cancelled || rows.length === 0) return;
            after = rows[rows.length - 1].sequence;
            raw += rows.map((row) => row.data).join("");
            setLines(capLines(parseAnsi(raw)));
          }
        } catch (cause: unknown) {
          if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
        }
      });
    };
    load();

    const stop = api.subscribeEvents(
      0,
      (event) => {
        if (event.task_id === id && event.kind === "output") load();
      },
      () => {},
    );
    return () => {
      cancelled = true;
      stop();
    };
  }, [id]);

  // 새 줄이 붙으면 바닥에 붙어 있던 경우에만 따라 내려간다.
  useEffect(() => {
    const container = containerRef.current;
    if (container && stickToBottom.current) container.scrollTop = container.scrollHeight;
  }, [lines]);

  if (error) return <Empty>출력을 불러오지 못했습니다. {error}</Empty>;
  if (!lines) return <Spinner label="출력을 불러오는 중" />;
  if (lines.length === 0) return <Empty>아직 출력이 없습니다.</Empty>;

  return (
    <div
      ref={containerRef}
      onScroll={(event) => {
        const el = event.currentTarget;
        stickToBottom.current =
          el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK_PX;
      }}
      className="h-full overflow-auto bg-black px-3 py-2"
    >
      <pre className="w-max min-w-full font-code text-xs leading-5">
        {lines.map((line, index) => (
          <div key={index}>
            {line.length === 0 ? (
              " "
            ) : (
              line.map((segment, part) => (
                <span
                  key={part}
                  style={{
                    color: segment.fg,
                    backgroundColor: segment.bg,
                    fontWeight: segment.bold ? 600 : undefined,
                    opacity: segment.dim ? 0.6 : undefined,
                    textDecoration: segment.underline ? "underline" : undefined,
                  }}
                >
                  {segment.text}
                </span>
              ))
            )}
          </div>
        ))}
      </pre>
    </div>
  );
}
