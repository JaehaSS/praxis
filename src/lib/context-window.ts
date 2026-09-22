/** 벤더별 컨텍스트 윈도 티어(토큰, 오름차순).
 *  claude는 200K가 표준이지만 `[1m]` 모델에서 1M로 열린다 — 단일 값이 아니라 티어로 두고
 *  모델 표기로 고른다. codex 272K(gpt-5 계열 입력 상한), agy/gemini 1M. */
const TIERS: Record<string, readonly number[]> = {
  claude: [200_000, 1_000_000],
  codex: [272_000],
  agy: [1_000_000],
  antigravity: [1_000_000],
  gemini: [1_000_000],
};

const DEFAULT_TIERS: readonly number[] = [200_000];

/** Claude Code의 1M 컨텍스트 표기 — `opus[1m]`·`claude-opus-5[1m]`처럼 모델 뒤에 붙는다. */
const LONG_CONTEXT = /\[1m\]/i;

const tiersFor = (agent: string | null | undefined): readonly number[] =>
  TIERS[(agent ?? "").trim()] ?? DEFAULT_TIERS;

/** 유효 컨텍스트 윈도 — 모델 표기만으로 고른다. */
export const contextWindowFor = (
  agent: string | null | undefined,
  model?: string | null,
): number => {
  const tiers = tiersFor(agent);
  const top = tiers[tiers.length - 1];
  return model != null && LONG_CONTEXT.test(model) ? top : tiers[0];
};

/** 컨텍스트 사용률(0~100 정수). 관측값이 없으면 null — 표시 자체를 숨긴다. */
export const contextPercent = (
  tokens: number | null,
  agent: string | null | undefined,
  model?: string | null,
  window?: number | null,
): number | null => {
  if (tokens == null || tokens <= 0) return null;
  const denominator = window ?? contextWindowFor(agent, model);
  if (!Number.isSafeInteger(denominator) || denominator <= 0) return null;
  return Math.min(100, Math.round((tokens / denominator) * 100));
};

/** 모델 전환이 **컨텍스트 윈도를 좁혀** 지금 쌓인 양을 못 담게 되면 경고 문구, 아니면 null.
 *
 *  `[1m]` 세션에서 200K 모델로 내려갈 때 이미 200K를 넘겼다면 다음 턴이 그대로 실패한다.
 *  경고는 세 관문을 모두 통과할 때만 나온다. 하나라도 빠지면 오탐이 일상이 되고, 매번 뜨는
 *  확인 창은 읽지 않고 누르는 창이 된다.
 *  ① 관측이 있어야 한다 — 없으면 판단 근거가 없다.
 *  ② 대상 모델을 해석할 수 있어야 한다 — 빈 값("벤더 기본")은 설정에 달려 있어 여기서 알 수
 *     없다. 모르는 것을 "좁다"고 단정하지 않는다(설정 기본이 `[1m]`이면 오히려 넓어진다).
 *  ③ 실제로 좁아져야 한다 — 단일 티어 벤더(codex 272K, `TIERS`에 없어 200K로 떨어지는
 *     커스텀 CLI)에서는 어떤 전환도 윈도를 바꾸지 않는다. 현재 모델과 견주지 않으면 그런
 *     세션은 관측이 티어를 넘는 순간부터 전환마다 경고를 맞는다. */
export const contextShrinkWarning = (
  agent: string | null | undefined,
  nextModel: string,
  currentModel: string | null | undefined,
  observedTokens: number | null | undefined,
): string | null => {
  if (observedTokens == null || observedTokens <= 0) return null;
  if (nextModel.trim().length === 0) return null;
  const next = contextWindowFor(agent, nextModel);
  if (next >= contextWindowFor(agent, currentModel)) return null;
  if (observedTokens <= next) return null;
  return (
    `이 세션은 이미 ${observedTokens.toLocaleString()} 토큰을 쓰고 있는데 ` +
    `바꾸려는 모델의 컨텍스트 윈도는 ${next.toLocaleString()} 토큰입니다.\n\n` +
    `전환하면 다음 턴이 실패할 수 있습니다. 계속할까요?`
  );
};
