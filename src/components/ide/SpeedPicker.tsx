import { useEffect, useState } from "react";
import { codexSpeedModels, type ServiceTier } from "../../lib/ipc";

interface Props {
  model: string;
  value: ServiceTier | null;
  onChange: (value: ServiceTier) => void;
  disabled?: boolean;
}

/** The selected setting applies to the next turn, not an assertion about server allocation. */
export function SpeedPicker({ model, value, onChange, disabled = false }: Props) {
  const [models, setModels] = useState<string[] | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    let alive = true;
    codexSpeedModels().then(
      (next) => { if (alive) setModels(next); },
      () => { if (alive) { setModels([]); setError(true); } },
    );
    return () => { alive = false; };
  }, [model]);
  const supported = models?.includes(model.trim()) ?? false;
  const hint = error ? "Fast 지원 정보를 읽지 못했습니다. Standard는 사용할 수 있습니다."
    : !model.trim() ? "Fast를 사용하려면 지원 모델을 직접 선택해 주세요."
    : models == null ? "Fast 지원 모델 확인 중…"
    : !supported ? "이 모델의 Fast 지원을 확인하지 못했습니다."
    : "다음 메시지부터 적용 · Fast는 더 빠른 응답을 제공하며 사용량이 증가합니다.";
  return (
    <select
      aria-label="Codex 실행 속도"
      title={hint}
      value={value ?? ""}
      disabled={disabled}
      onChange={(event) => {
        const tier = event.target.value;
        if (tier === "default" || (tier === "fast" && supported)) onChange(tier);
      }}
      className={`max-w-[132px] rounded-md border bg-surface px-2 py-1 text-xs disabled:opacity-50 ${value === "fast" ? "border-primary/50 text-primary-bright" : "border-border text-text-secondary"}`}
    >
      {value == null && <option value="" disabled>Codex 기본값</option>}
      <option value="default">Standard</option>
      <option value="fast" disabled={!supported}>⚡ Fast</option>
    </select>
  );
}
