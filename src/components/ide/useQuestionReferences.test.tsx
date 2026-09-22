// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { useQuestionReferences } from "./useQuestionReferences";
import type { QuestionReference } from "../../lib/side-question";
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("keeps reference snapshots scoped and preserves edits and additions during admission", async () => {
  const node = document.createElement("div");
  const root = createRoot(node);
  let current!: ReturnType<typeof useQuestionReferences>;
  function Probe({ scope }: { scope: string }) { current = useQuestionReferences(scope); return null; }
  const ref: QuestionReference = { id: "one", sourceKey: "local:1", question: "why", text: "selected", contexts: [], incomplete: false };
  try {
    await act(async () => root.render(<Probe scope="local:1" />));
    await act(async () => { current.attach(ref); current.attach(ref); });
    expect(current.references).toHaveLength(1);
    const submitted = structuredClone(current.references);
    await act(async () => current.setReferences([{ ...ref, text: "edited while waiting" }, { ...ref, id: "two" }]));
    await act(async () => root.render(<Probe scope="remote:1" />));
    expect(current.references).toEqual([]);
    await act(async () => current.consume("local:1", submitted));
    await act(async () => root.render(<Probe scope="local:1" />));
    expect(current.references.map((item) => item.text)).toEqual(["edited while waiting", "selected"]);
    const final = structuredClone(current.references);
    await act(async () => current.consume("local:1", final));
    expect(current.references).toEqual([]);
  } finally { await act(async () => root.unmount()); }
});
