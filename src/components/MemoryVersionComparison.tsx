import { DiffEditor } from "@monaco-editor/react";
import { useState, type ReactElement } from "react";
import type { Memory, MemoryVersion } from "../lib/ipc";
import {
  canRestoreMemoryVersion,
  defaultComparedVersion,
} from "./memory-version";

interface Props {
  memory: Memory;
  versions: MemoryVersion[];
  onRestore: (version: MemoryVersion) => Promise<void>;
  busy: boolean;
}

function VersionButtons({
  memory,
  versions,
  selected,
  onSelect,
}: {
  memory: Memory;
  versions: MemoryVersion[];
  selected: number;
  onSelect: (version: number) => void;
}): ReactElement {
  return (
    <div className="flex flex-wrap gap-1">
      {versions.map((row) => (
        <button
          key={row.version}
          className={row.version === selected ? "text-primary-bright" : ""}
          onClick={() => onSelect(row.version)}
        >
          v{row.version}{row.version === memory.current_version ? " · 현재" : ""}
        </button>
      ))}
    </div>
  );
}

function VersionMeta({
  memory,
  selected,
  busy,
  onRestore,
}: {
  memory: Memory;
  selected: MemoryVersion;
  busy: boolean;
  onRestore: (version: MemoryVersion) => Promise<void>;
}): ReactElement {
  return (
    <div className="flex flex-wrap items-center gap-2 text-text-muted">
      <span>v{selected.version} → 현재 v{memory.current_version}</span>
      <span>{selected.knowledge_type}</span>
      <span>과거 근거 {selected.evidence_count}건</span>
      <span>{selected.editor_kind}</span>
      {canRestoreMemoryVersion(memory, selected) && (
        <button
          className="ml-auto text-primary-bright"
          disabled={busy}
          onClick={() => void onRestore(selected)}
        >
          이 버전을 새 후보로 복원
        </button>
      )}
    </div>
  );
}

export function MemoryVersionComparison({
  memory,
  versions,
  onRestore,
  busy,
}: Props): ReactElement {
  const initial = defaultComparedVersion(memory, versions);
  const [selectedNumber, setSelectedNumber] = useState(initial?.version ?? null);
  const selected = versions.find((row) => row.version === selectedNumber)
    ?? initial;
  const current = versions.find((row) => row.version === memory.current_version);
  if (!selected) return <div className="text-text-muted">저장된 버전이 없습니다.</div>;
  return (
    <div className="flex flex-col gap-2">
      <VersionButtons
        memory={memory}
        versions={versions}
        selected={selected.version}
        onSelect={setSelectedNumber}
      />
      <VersionMeta
        memory={memory}
        selected={selected}
        busy={busy}
        onRestore={onRestore}
      />
      <DiffEditor
        height="240px"
        language="plaintext"
        original={selected.content}
        modified={current?.content ?? memory.content}
        options={{ readOnly: true, renderSideBySide: true, minimap: { enabled: false } }}
      />
    </div>
  );
}
