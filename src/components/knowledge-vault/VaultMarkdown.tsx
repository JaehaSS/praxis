import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export function VaultMarkdown({ body }: { body: string }) {
  return <div className="min-w-0 max-w-[42.5rem] break-words text-[16px] leading-[1.625] text-text [&_ol]:my-3 [&_ol]:list-decimal [&_ol]:pl-6 [&_p]:my-3 [&_table]:border-collapse [&_td]:border [&_td]:border-border [&_td]:p-2 [&_th]:border [&_th]:border-border [&_th]:p-2 [&_ul]:my-3 [&_ul]:list-disc [&_ul]:pl-6">
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      skipHtml
      components={{
        a: ({ children }) => <span>{children}</span>,
        img: ({ alt }) => <span className="text-text-secondary">[이미지: {alt || "표시하지 않음"}]</span>,
        h1: ({ children }) => <h1 className="mb-3 text-2xl font-semibold">{children}</h1>,
        h2: ({ children }) => <h2 className="mt-5 mb-2 text-xl font-semibold">{children}</h2>,
        h3: ({ children }) => <h3 className="mt-4 mb-2 text-lg font-semibold">{children}</h3>,
        pre: ({ children }) => <pre className="my-3 overflow-auto rounded-md bg-raised p-3 font-code text-xs">{children}</pre>,
        table: ({ children }) => <div className="my-3 overflow-auto"><table>{children}</table></div>,
      }}
    >
      {body}
    </ReactMarkdown>
  </div>;
}
