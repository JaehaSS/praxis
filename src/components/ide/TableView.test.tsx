import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { TableView, parseTablePreview, tableSummary } from "./TableView";

const preview = (over: Record<string, unknown> = {}) =>
  JSON.stringify({
    format: "parquet",
    columns: [
      { name: "id", type: "int64" },
      { name: "name", type: "string" },
    ],
    rows: [
      ["1", "abc"],
      ["2", null],
    ],
    total_rows: 2,
    total_columns: 2,
    row_groups: 1,
    shown_rows: 2,
    shown_columns: 2,
    error: null,
    ...over,
  });

describe("표 파일 미리보기", () => {
  it("컬럼 이름·타입과 셀을 그리고 null은 null로 표시한다", () => {
    const html = renderToStaticMarkup(<TableView content={preview()} name="a.parquet" />);
    expect(html).toContain("id");
    expect(html).toContain("int64");
    expect(html).toContain("abc");
    expect(html).toContain(">null<");
    expect(html).toContain("a.parquet");
  });

  it("잘린 표는 전체 대비 얼마를 보이는지 말한다", () => {
    const t = parseTablePreview(
      preview({ total_rows: 1000000, shown_rows: 200, total_columns: 150, shown_columns: 100 }),
    );
    expect(tableSummary(t)).toBe("총 1,000,000행 중 처음 200행 · 컬럼 150개 중 100개 · row group 1");
  });

  it("잘리지 않았으면 잘렸다는 말을 하지 않는다", () => {
    expect(tableSummary(parseTablePreview(preview()))).toBe("2행 · 컬럼 2개 · row group 1");
  });

  it("백엔드가 error를 실어 보내면 표 위에 띄운다", () => {
    const html = renderToStaticMarkup(
      <TableView content={preview({ error: "Parquet 파일이 아닙니다", rows: [], columns: [] })} name="x.parquet" />,
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain("Parquet 파일이 아닙니다");
  });

  it("JSON이 아니면 탭을 죽이지 않고 오류만 담는다", () => {
    const t = parseTablePreview("not json");
    expect(t.error).toContain("미리보기를 읽지 못했습니다");
    expect(t.rows).toEqual([]);
  });

  it("콘솔 열기 콜백이 있을 때만 IPython 버튼을 그린다", () => {
    expect(renderToStaticMarkup(<TableView content={preview()} name="a.parquet" />)).not.toContain(
      "IPython으로 열기",
    );
    expect(
      renderToStaticMarkup(<TableView content={preview()} name="a.parquet" onOpenInRepl={() => {}} />),
    ).toContain("IPython으로 열기");
  });
});
