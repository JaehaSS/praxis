import { useState } from "react";
import { DirectoryBrowser } from "./DirectoryBrowser";

interface Props {
  /** 처음 열 경로 — 보통 현재 선택된 레포. */
  initialPath?: string;
  title?: string;
  onPick: (path: string) => void;
  onClose: () => void;
}

/** 원격 Runner의 디렉터리를 훑어 작업 대상 폴더를 고르는 모달.
 *
 * 로컬은 네이티브 폴더 다이얼로그가 있지만 원격에는 없다 — 같은 자리에서 같은 일을
 * 하도록 `DirectoryBrowser`를 그대로 띄운다. git 레포가 아닌 폴더도 고를 수 있다. */
export function DirectoryPickerModal({ initialPath, title = "원격 폴더 선택", onPick, onClose }: Props) {
  const [current, setCurrent] = useState(initialPath ?? "");

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-[640px] max-w-[90vw] h-[70vh] flex flex-col rounded-lg border border-border-strong bg-bg shadow-xl"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-label={title}
      >
        <div className="px-4 py-2.5 border-b border-border text-sm text-text shrink-0">
          {title}
        </div>
        <DirectoryBrowser
          initialPath={initialPath}
          onPathChange={setCurrent}
          className="flex-1"
          renderEntryAction={(entry) =>
            entry.is_dir && !entry.denied ? (
              <button
                className="text-xs text-text-muted hover:text-primary-bright"
                onClick={() => onPick(entry.path)}
                title={`${entry.path} 선택`}
              >
                선택
              </button>
            ) : null
          }
        />
        <div className="flex items-center justify-between gap-3 px-4 py-2.5 border-t border-border shrink-0">
          <span className="min-w-0 truncate text-xs font-code text-text-muted" title={current}>
            {current}
          </span>
          <div className="flex items-center gap-2 shrink-0">
            <button
              className="h-8 px-3 rounded-md border border-border text-text-secondary hover:border-border-strong"
              onClick={onClose}
            >
              취소
            </button>
            <button
              className="h-8 px-3 rounded-md border border-border text-text hover:border-border-strong disabled:opacity-50"
              disabled={!current}
              onClick={() => onPick(current)}
            >
              이 폴더 선택
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
