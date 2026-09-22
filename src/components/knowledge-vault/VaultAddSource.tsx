import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { vaultCreateTextSource, vaultCreateUrlSource, vaultImportFiles } from "../../lib/knowledge-vault-ipc";
import { LOCAL_HOST, type HostId } from "../../lib/transport";
import { vaultButton, vaultCard, vaultError, vaultInput, vaultLabel, vaultTab, vaultTextarea } from "./ui";

interface Props {
  vaultId: string;
  onNote: (title: string, body: string) => Promise<void>;
  host?: HostId;
  onSaved?: (revisionIds: string[]) => Promise<void>;
  disabled?: boolean;
}

export function VaultAddSource({ vaultId, onNote: _onNote, host = LOCAL_HOST, onSaved, disabled = false }: Props) {
  const [openDialog, setOpenDialog] = useState(false);
  const [mode, setMode] = useState<"file" | "text" | "url">("text");
  const [paths, setPaths] = useState<string[]>([]);
  const [title, setTitle] = useState(""); const [body, setBody] = useState(""); const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  const dialog = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!openDialog) return;
    const close = (event: KeyboardEvent) => { if (event.key === "Escape" && !busy) setOpenDialog(false); if (event.key === "Tab") trapFocus(event, dialog.current); };
    document.addEventListener("keydown", close); dialog.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => document.removeEventListener("keydown", close);
  }, [busy, openDialog]);
  const pick = async () => { const selected = await open({ multiple: true, directory: false }); if (selected) setPaths(Array.isArray(selected) ? selected : [selected]); };
  const save = async () => {
    if (busy) return;
    setBusy(true); setError("");
    try {
      const results = mode === "file" ? await vaultImportFiles(vaultId, paths, "private-data", undefined, host) : [mode === "text" ? await vaultCreateTextSource(vaultId, title, body, "private-data", undefined, host) : await vaultCreateUrlSource(vaultId, title, url, body, "private-data", undefined, host)];
      const revisions = results.flatMap(result => result && "source" in result ? (result.source ? [result.source.revision_id] : []) : result ? [result.revision_id] : []);
      const failures = results.flatMap(result => result && "error" in result && result.error ? [result.error] : []);
      if (failures.length) setError(`일부 자료를 추가하지 못했습니다. ${failures.join(" ")}`);
      if (revisions.length) await onSaved?.(revisions);
      if (mode === "file" && failures.length) setPaths(results.flatMap(result => "error" in result && result.error ? [result.path] : []));
      if (!failures.length) setOpenDialog(false);
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  return <>
    <button className={vaultButton} disabled={disabled} type="button" onClick={() => setOpenDialog(true)}>자료 추가</button>
    {openDialog && <div aria-label="자료 추가" aria-modal="true" className="fixed inset-0 z-20 grid place-items-center bg-black/40 p-4" ref={dialog} role="dialog">
      <section className={`${vaultCard} w-full max-w-xl space-y-3`}><div className="flex items-center justify-between"><h2 className="font-medium">자료 추가</h2><button className={vaultButton} disabled={busy} type="button" onClick={() => setOpenDialog(false)}>닫기</button></div>
        <nav className="flex gap-1" aria-label="자료 종류">{(["text", "file", "url"] as const).map(item => <button aria-pressed={mode === item} className={vaultTab} key={item} type="button" onClick={() => setMode(item)}>{item === "text" ? "텍스트" : item === "file" ? "파일" : "URL"}</button>)}</nav>
        {mode === "file" ? <div className="space-y-2"><button className={vaultButton} disabled={busy} type="button" onClick={() => void pick()}>파일 고르기</button><p className="break-all whitespace-pre-wrap text-sm text-text-secondary">{paths.length ? paths.join("\n") : "선택한 파일이 없습니다."}</p></div> : <><label className={vaultLabel}>제목<input className={vaultInput} disabled={busy} value={title} onChange={event => setTitle(event.target.value)} /></label>{mode === "url" && <label className={vaultLabel}>URL<input className={vaultInput} disabled={busy} value={url} onChange={event => setUrl(event.target.value)} /></label>}<label className={vaultLabel}>{mode === "url" ? "메모" : "내용"}<textarea className={vaultTextarea} disabled={busy} value={body} onChange={event => setBody(event.target.value)} /></label></>}
        <button className={vaultButton} disabled={busy || (mode === "file" ? !paths.length : !title.trim() || (mode === "url" && !url.trim()))} type="button" onClick={() => void save()}>{busy ? "추가 중…" : "추가"}</button>{error && <p className={vaultError} role="alert">{error}</p>}
      </section>
    </div>}
  </>;
}

function trapFocus(event: KeyboardEvent, dialog: HTMLDivElement | null) {
  const focusable = dialog?.querySelectorAll<HTMLElement>("button, input, textarea, select");
  if (!focusable?.length) return;
  const first = focusable[0]; const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
  if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
}
