import { useState } from "react";
import type { TaskRef } from "../lib/transport";
import {
  annotationSave,
  annotationsResend,
  type RematchedAnnotation,
} from "../lib/ipc";
import { draftAnnotationIds } from "../lib/annotations";

interface AnnotationInput {
  hunk_id: string;
  line: number;
  side: string;
  body_md: string;
}

export function useDiffReviewActions(
  task: TaskRef,
  selectedPath: string | null,
  annotations: RematchedAnnotation[],
  refreshAnnotations: () => void,
) {
  const [resending, setResending] = useState(false);
  const [resendError, setResendError] = useState<string | null>(null);
  const draftIds = draftAnnotationIds(annotations);

  const create = async (input: AnnotationInput): Promise<void> => {
    // 경로 없이 저장하지 않는다 — 빈 경로의 주석은 어느 파일에도 다시 붙지 않는 영구
    // 쓰레기가 된다. 상태가 어긋났다면 아무 일도 일어나지 않는 편이 낫다.
    if (selectedPath == null) return;
    await annotationSave(task, { ...input, path: selectedPath });
    refreshAnnotations();
  };
  const update = async (id: string, body_md: string): Promise<void> => {
    const target = annotations.find((annotation) => annotation.id === id);
    if (!target) return;
    await annotationSave(task, {
      id: target.id,
      hunk_id: target.hunk_id,
      path: target.path,
      line: target.line,
      side: target.side,
      body_md,
    });
    refreshAnnotations();
  };
  const resend = async (): Promise<void> => {
    setResending(true);
    setResendError(null);
    try {
      await annotationsResend(task, draftIds);
      refreshAnnotations();
    } catch (cause) {
      setResendError(String(cause));
    } finally {
      setResending(false);
    }
  };

  return { create, draftIds, resend, resendError, resending, update };
}
