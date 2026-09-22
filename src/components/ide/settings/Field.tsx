import type React from "react";

/** 설정 화면의 입력 한 칸이 공유하는 테두리·여백. */
export const fieldInput =
  "w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text outline-none focus:border-primary";

/** 라벨 + 보조 설명을 붙인 입력 한 칸. */
export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="mb-1 flex items-baseline gap-2">
        <span className="text-xs text-text-secondary">{label}</span>
        {hint && <span className="text-[11px] text-text-muted">{hint}</span>}
      </span>
      {children}
    </label>
  );
}
