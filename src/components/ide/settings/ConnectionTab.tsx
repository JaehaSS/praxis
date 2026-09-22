import { AgentHealthSection } from "../AgentHealthSection";
import { UsageTokenSection } from "../UsageTokenSection";
import { SettingSection, SettingsTabShell } from "./SettingRow";

/** 연결 — 에이전트 CLI 상태. 폰 연결은 관리 묶음의 "모바일" 탭이 맡는다. */
export function ConnectionTab() {
  return (
    <SettingsTabShell>
      <SettingSection
        id="agent-cli"
        title="에이전트 CLI"
        hint="CLI에 직접 물어 읽는다 — 자격증명 파일 포맷은 벤더가 예고 없이 바꾸지만 CLI 계약은 안정적이다. 인증·버전은 열 때마다 다시 확인하고, 최신 버전 조회만 6시간 캐시한다."
      >
        <AgentHealthSection />
        <UsageTokenSection />
      </SettingSection>
    </SettingsTabShell>
  );
}
