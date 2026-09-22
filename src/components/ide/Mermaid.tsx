import { useEffect, useId, useState } from "react";
import { CodeCopyButton } from "./CodeCopyButton";

interface MermaidProps {
  chart: string;
  dark: boolean;
}

/** Mermaid 코드블록 → SVG 렌더러.
 *  mermaid는 렌더 시점에 동적 import(번들 분할 + 초기 로드 절감).
 *  id는 useId() 기반 결정적 값 — Math.random()/Date.now() 금지(StrictMode 이중렌더 안전). */
export function Mermaid({ chart, dark }: MermaidProps) {
  const rawId = useId().replace(/[:]/g, "");
  const id = `mmd-${rawId}`;
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function render() {
      try {
        const mermaid = (await import("mermaid")).default;
        mermaid.initialize({
          startOnLoad: false,
          theme: dark ? "dark" : "default",
          securityLevel: "strict",
        });
        const result = await mermaid.render(id, chart);
        if (cancelled) return;
        setSvg(result.svg);
        setError(null);
      } catch (err) {
        if (cancelled) return;
        setSvg(null);
        setError(err instanceof Error ? err.message : String(err));
      }
    }

    void render();
    return () => {
      cancelled = true;
    };
  }, [chart, dark, id]);

  if (error) {
    return (
      <div className="relative my-2">
        <CodeCopyButton text={chart} />
        <div className="mb-1 text-xs text-status-failed">Mermaid 렌더 실패: {error}</div>
        <pre className="rounded-md bg-bg border border-border overflow-auto text-xs font-code px-2.5 py-2 whitespace-pre text-text">
          {chart}
        </pre>
      </div>
    );
  }

  if (!svg) return null;

  return (
    <div className="relative my-2">
      <CodeCopyButton text={chart} />
      <div
        className="overflow-auto rounded-md border border-border bg-bg p-2"
        // eslint-disable-next-line react/no-danger -- mermaid.render(securityLevel: 'strict') 신뢰 SVG 출력
        dangerouslySetInnerHTML={{ __html: svg }}
      />
    </div>
  );
}
