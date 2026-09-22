import { useEffect, useRef, useState } from "react";
import { vaultSettingsGet, vaultSettingsSet } from "../../lib/knowledge-vault-ipc";
import { LOCAL_HOST, type HostId } from "../../lib/transport";
import { vaultButton, vaultCard, vaultError, vaultInput, vaultLabel } from "./ui";

/** 저장 직후 안내 — 위키 폴더가 바뀌면 같은 파일이 다른 갈래로 분류된다. 재스캔 전까지 목록은 옛 분류다. */
const RESCAN = "저장했습니다. 위키 폴더가 바뀌면 문서 분류가 달라집니다 — 관리의 새로 고침으로 다시 스캔하세요.";

/**
 * 창고 폴더 설정 — 무엇이 위키 문서인지(`wiki_dir`), 정리 세션이 부를 스킬(`organizer_skill`),
 * 위키를 열 때 먼저 띄울 진입 문서(`wiki_home`). 셋 다 창고 하나가 아니라 앱 설정이라
 * 자료함 연결과 따로 산다.
 */
export function VaultFolderSettings({ host, onSaved }: { host: HostId; onSaved?: () => void }) {
  const [wikiDir, setWikiDir] = useState("");
  const [skill, setSkill] = useState("");
  const [home, setHome] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const request = useRef(0);
  useEffect(() => {
    const current = ++request.current;
    if (host !== LOCAL_HOST) { setLoading(false); return; }
    setLoading(true); setError("");
    void vaultSettingsGet(host)
      .then(settings => { if (current !== request.current) return; setWikiDir(settings.wiki_dir); setSkill(settings.organizer_skill); setHome(settings.wiki_home); })
      .catch(reason => { if (current === request.current) setError(String(reason)); })
      .finally(() => { if (current === request.current) setLoading(false); });
  }, [host]);
  if (host !== LOCAL_HOST) return null;
  const save = async () => {
    if (busy) return;
    setBusy(true); setError(""); setNotice("");
    try {
      const saved = await vaultSettingsSet(wikiDir.trim(), skill.trim(), home.trim(), host);
      setWikiDir(saved.wiki_dir); setSkill(saved.organizer_skill); setHome(saved.wiki_home); setNotice(RESCAN); onSaved?.();
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  return <section aria-label="창고 폴더 설정" className={`${vaultCard} space-y-3`}>
    <h2 className="font-medium">창고 폴더</h2>
    {loading ? <p className="text-sm text-text-secondary">불러오는 중…</p> : <>
      <label className={vaultLabel}>위키 폴더 (창고 루트 기준 상대 경로)
        <input aria-label="위키 폴더" className={vaultInput} disabled={busy} placeholder="문서/기술-위키/wiki" value={wikiDir} onChange={event => { setWikiDir(event.target.value); setNotice(""); }} />
      </label>
      <p className="text-xs text-text-secondary">이 폴더 아래의 <span className="font-code">*.md</span>가 위키 문서가 됩니다. 비우면 <span className="font-code">wiki</span>를 씁니다.</p>
      <label className={vaultLabel}>진입 문서
        <input aria-label="진입 문서" className={vaultInput} disabled={busy} placeholder="위키-시작.md" value={home} onChange={event => { setHome(event.target.value); setNotice(""); }} />
      </label>
      <p className="text-xs text-text-secondary">위키를 열면 이 문서를 먼저 띄웁니다. 파일 이름만 적으면 위키 폴더 어디에 있든 찾습니다. 없으면 역링크가 가장 많은 문서로 갑니다. 비우면 <span className="font-code">위키-시작.md</span>를 씁니다.</p>
      <label className={vaultLabel}>정리 스킬
        <input aria-label="정리 스킬" className={vaultInput} disabled={busy} placeholder="knowledge-harness" value={skill} onChange={event => { setSkill(event.target.value); setNotice(""); }} />
      </label>
      <p className="text-xs text-text-secondary">정리 세션이 부를 <span className="font-code">~/.claude/skills</span> 아래 스킬 이름입니다. 비우면 <span className="font-code">wiki-organizer</span>를 씁니다.</p>
      <button className={vaultButton} disabled={busy} type="button" onClick={() => void save()}>저장</button>
      {notice && <p className="text-sm text-text-secondary" role="status">{notice}</p>}
    </>}
    {error && <p className={vaultError} role="alert">{error}</p>}
  </section>;
}
