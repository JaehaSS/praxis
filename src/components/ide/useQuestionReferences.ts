import { useCallback, useState } from "react";
import type { QuestionReference } from "../../lib/side-question";

/** Draft attachments are snapshots owned by the parent session coordinate, never a global task id. */
export function useQuestionReferences(key: string | null) {
  const [bySession, setBySession] = useState<Map<string, QuestionReference[]>>(() => new Map());
  const references = key == null ? [] : bySession.get(key) ?? [];
  const setReferences = useCallback((next: QuestionReference[]) => {
    if (key == null) return;
    setBySession((previous) => new Map(previous).set(key, next));
  }, [key]);
  const attach = useCallback((reference: QuestionReference) => {
    if (key == null) return;
    setBySession((previous) => {
      const current = previous.get(key) ?? [];
      if (current.some((item) => item.id === reference.id)) return previous;
      return new Map(previous).set(key, [...current, structuredClone(reference)]);
    });
  }, [key]);
  const consume = useCallback((owner: string, submitted: QuestionReference[]) => {
    setBySession((previous) => {
      const current = previous.get(owner) ?? [];
      // An edited attachment with the same id is a new draft version and must survive.
      const remaining = current.filter((item) => !submitted.some((sent) => JSON.stringify(sent) === JSON.stringify(item)));
      return new Map(previous).set(owner, remaining);
    });
  }, []);
  return { references, setReferences, attach, consume };
}
