import { useEffect, useState } from "react";

import {
  memoryFileOpen,
  memoryFilesList,
  memorySessionOpen,
  memorySettingsGet,
  memorySettingsSet,
  type MemoryFileInfo,
  type MemorySettings,
} from "../../lib/memory-file-ipc";
import { LOCAL_HOST, type HostId } from "../../lib/transport";
import { vaultButton, vaultCard, vaultError, vaultInput, vaultPrimaryButton } from "./ui";

interface Props {
  agent?: string;
  host: HostId;
}

/** 스킬 이름은 설정값이다 — 세션을 연 백엔드가 실제로 부른 이름을 돌려주므로 그것을 그대로 읽는다. */
const skillMissing = (skill: string) => `~/.claude/skills/${skill} 스킬이 없습니다. 세션에서 규칙을 직접 지시하거나 스킬을 먼저 두세요.`;
const INTRO = "에이전트가 작업 중 직접 쓴다. 작업 시작 때 AGENTS.md 블록으로 실린다.";
const OVER_CAP = "상한 초과 — 넘긴 줄은 실리지 않는다";

/**
 * 메모리 파일 목록 — 본문 편집기는 여기에 없다. 정본은 파일이고 편집은 에디터 창이 한다(설계 §5).
 * 창고 연결과 무관하게 동작한다 — 메모리 루트는 창고가 없으면 앱 데이터 디렉터리로 떨어진다(R7).
 */
export function VaultMemoryPanel({ agent = "claude", host }: Props) {
  const [files, setFiles] = useState<MemoryFileInfo[]>([]);
  const [settings, setSettings] = useState<MemorySettings | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [loading, setLoading] = useState(true);
  const [sessionOpening, setSessionOpening] = useState(false);
  const [saving, setSaving] = useState(false);
  const [form, setForm] = useState<MemorySettings | null>(null);

  const load = async () => {
    if (host !== LOCAL_HOST) { setLoading(false); return; }
    setLoading(true);
    try {
      const [nextFiles, nextSettings] = await Promise.all([memoryFilesList(host), memorySettingsGet(host)]);
      setFiles(nextFiles);
      setSettings(nextSettings);
      setForm(current => current ?? nextSettings);
      setError("");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { void load(); }, [host]);

  const openSession = async () => {
    if (sessionOpening) return;
    setSessionOpening(true);
    try {
      const opened = await memorySessionOpen(agent, host);
      const message = opened.skill_present === false ? skillMissing(opened.skill) : opened.warning;
      setNotice(message ?? "");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSessionOpening(false);
    }
  };

  const open = async (path: string) => {
    try { await memoryFileOpen(path, host); setError(""); }
    catch (reason) { setError(String(reason)); }
  };

  const save = async () => {
    if (!form || saving) return;
    setSaving(true);
    try {
      const saved = await memorySettingsSet(form, host);
      setSettings(saved);
      setForm(saved);
      setError("");
      await load();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSaving(false);
    }
  };

  if (host !== LOCAL_HOST) return <p className={vaultCard}>메모리 파일은 로컬 세션에서만 사용할 수 있습니다.</p>;

  return <section aria-label="메모리" className="space-y-4">
    <div className={`${vaultCard} space-y-2`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="min-w-0 truncate font-mono text-sm" title={settings?.effective_root ?? ""}>{settings?.effective_root ?? "메모리 루트 확인 중…"}</p>
        <button className={vaultPrimaryButton} disabled={sessionOpening} type="button" onClick={() => void openSession()}>정리 세션 열기</button>
      </div>
      <p className="text-sm text-text-secondary">{INTRO}</p>
      {notice && <p className="text-sm text-text-secondary" role="status">{notice}</p>}
    </div>

    {error && <p className={vaultError} role="alert">{error}</p>}

    <div className={`${vaultCard} min-w-0`}>
      {loading
        ? <p aria-busy="true" className="text-sm text-text-secondary">불러오는 중…</p>
        : files.length
          ? <div className="divide-y divide-border">{files.map(file => <MemoryRow file={file} key={file.path} onOpen={open} />)}</div>
          : <p className="text-sm text-text-secondary">메모리 파일이 아직 없습니다. 에이전트가 작업 중 위 경로에 만듭니다.</p>}
    </div>

    <details className={`${vaultCard} min-w-0`}>
      <summary className="cursor-pointer text-sm font-medium">설정</summary>
      {form && <div className="mt-3 space-y-3">
        <label className="grid gap-1 text-sm text-text-secondary">메모리 루트
          <input aria-label="메모리 루트" className={vaultInput} placeholder={settings?.effective_root ?? ""} value={form.root} onChange={event => setForm({ ...form, root: event.target.value })} />
        </label>
        <p className="text-xs text-text-muted">비워 두면 창고 루트의 memory/, 창고가 없으면 앱 데이터 디렉터리를 쓴다.</p>
        <div className="grid grid-cols-2 gap-3">
          <CapInput label="MEMORY.md 줄 상한" value={form.cap_lines} onChange={value => setForm({ ...form, cap_lines: value })} />
          <CapInput label="MEMORY.md 바이트 상한" value={form.cap_bytes} onChange={value => setForm({ ...form, cap_bytes: value })} />
          <CapInput label="USER.md 줄 상한" value={form.user_cap_lines} onChange={value => setForm({ ...form, user_cap_lines: value })} />
          <CapInput label="USER.md 바이트 상한" value={form.user_cap_bytes} onChange={value => setForm({ ...form, user_cap_bytes: value })} />
        </div>
        <button className={vaultButton} disabled={saving} type="button" onClick={() => void save()}>저장</button>
      </div>}
    </details>
  </section>;
}

function CapInput({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  return <label className="grid gap-1 text-sm text-text-secondary">{label}
    <input aria-label={label} className={vaultInput} min={1} type="number" value={value} onChange={event => onChange(Number(event.target.value))} />
  </label>;
}

function MemoryRow({ file, onOpen }: { file: MemoryFileInfo; onOpen: (path: string) => Promise<void> }) {
  const over = file.lines > file.cap_lines;
  const ratio = file.cap_lines > 0 ? Math.min(file.lines / file.cap_lines, 1) : 0;
  // 색은 토큰만 쓴다 — 하드코딩한 색은 테마 전환에서 그대로 남는다.
  const fill = over ? "var(--c-failed)" : ratio >= 0.8 ? "var(--c-awaiting)" : "var(--c-primary)";
  return <div className="min-w-0 space-y-1 py-3">
    <div className="flex flex-wrap items-center justify-between gap-2">
      <span className="text-sm font-medium">{rowLabel(file)}</span>
      <button className={vaultButton} type="button" onClick={() => void onOpen(file.path)}>{file.exists ? "열기" : "만들기"}</button>
    </div>
    <p className="min-w-0 truncate font-mono text-xs text-text-secondary" title={file.path}>{file.path}</p>
    <div aria-hidden="true" className="h-1.5 w-full rounded" style={{ background: "var(--c-empty)" }}>
      <div className="h-full rounded" style={{ background: fill, width: `${Math.round(ratio * 100)}%` }} />
    </div>
    <p className="text-xs text-text-secondary">{file.lines} / {file.cap_lines}줄 · {file.bytes} / {file.cap_bytes}B · 수정 {stamp(file.modified_at)} · 투영 {stamp(file.last_projected_at)}{file.last_task_id != null ? ` #${file.last_task_id}` : ""}</p>
    {over && <p className="text-xs text-status-failed">{OVER_CAP}</p>}
  </div>;
}

function rowLabel(file: MemoryFileInfo): string {
  if (file.kind === "user") return "전역 USER.md";
  const source = file.repo ?? file.repo_key ?? file.path;
  const parts = source.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? source;
}

/** 초 단위 unix 시각. 없으면 "없음" — 아직 한 번도 쓰이거나 실린 적 없다는 뜻이다. */
function stamp(seconds: number | null): string {
  if (seconds == null) return "없음";
  const date = new Date(seconds * 1000);
  return Number.isNaN(date.getTime()) ? "없음" : date.toLocaleString("ko-KR", { dateStyle: "short", timeStyle: "short" });
}
