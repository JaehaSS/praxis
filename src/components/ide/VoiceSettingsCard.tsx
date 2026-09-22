import { useEffect, useState } from "react";
import {
  voiceServerStart,
  voiceServerStatus,
  voiceServerStop,
  voiceSettingsGet,
  voiceSettingsSet,
  voiceSttTest,
  type VoiceServerStatus,
  type VoiceSettings,
} from "../../lib/ipc";
import { RESERVED_SHORTCUTS } from "../../lib/hotkey";
import { HotkeyInput, type HotkeyConflict } from "./HotkeyInput";

const FIELDS: {
  key: keyof VoiceSettings;
  label: string;
  hint?: string;
  type?: string;
  hotkey?: true;
}[] = [
  { key: "base_url", label: "서버 주소", hint: "OpenAI 호환 엔드포인트의 /v1 까지" },
  { key: "model", label: "모델" },
  { key: "api_key", label: "API 키", hint: "로컬 서버도 키를 요구한다", type: "password" },
  {
    key: "language",
    label: "언어",
    hint: "ISO 코드(whisper 계열 ko) 또는 로케일(ohr ko-KR). 비우면 서버가 자동 판별",
  },
  {
    key: "hotkey_command",
    label: "커맨드 핫키",
    hint: "칸을 누르고 키를 눌러 지정 — 홀드하는 동안 녹음(화면 전환·전송)",
    hotkey: true,
  },
  {
    key: "hotkey_dictation",
    label: "받아쓰기 핫키",
    hint: "칸을 누르고 키를 눌러 지정 — 홀드하는 동안 녹음(프롬프트 입력)",
    hotkey: true,
  },
];

/**
 * 이 칸이 가져갈 수 없는 조합 — 앱이 이미 쓰는 단축키와 나머지 한쪽 음성 핫키.
 * 두 음성 핫키가 같으면 커맨드가 먼저 매칭되어 받아쓰기는 영영 잡히지 않는다
 * (Rust `voice::validate` 도 같은 이유로 저장을 거부한다 — 여기서는 그 전에 알린다).
 */
const conflictsFor = (key: keyof VoiceSettings, settings: VoiceSettings): HotkeyConflict[] => {
  const other =
    key === "hotkey_command"
      ? { spec: settings.hotkey_dictation, owner: "받아쓰기 핫키" }
      : { spec: settings.hotkey_command, owner: "커맨드 핫키" };
  return other.spec.trim() ? [...RESERVED_SHORTCUTS, other] : [...RESERVED_SHORTCUTS];
};

type Status = { kind: "idle" | "ok" | "error"; message: string };

/** ohr(Apple SpeechAnalyzer 래퍼)로 갈아타는 한 벌. 핫키는 사용자 것이므로 건드리지 않는다. */
export const OHR_PRESET = {
  base_url: "http://127.0.0.1:11434/v1",
  model: "apple-speechanalyzer",
  api_key: "",
  language: "ko-KR",
};

const SERVER_POLL_MS = 3000;

const BUTTON_CLASS =
  "h-8 px-3 rounded-md border border-border text-text-secondary hover:border-border-strong disabled:opacity-50";

/** 상태 줄 한 문장. Badge 는 작업 상태 전용이라(DESIGN.md) 여기서는 색 클래스만 쓴다. */
const serverLine = (server: VoiceServerStatus): { tone: string; text: string } => {
  if (!server.installed)
    return {
      tone: "text-text-muted",
      text: "설치 안 됨 — runbook(docs/runbooks/voice-stt-server.md) 경로 A 참고",
    };
  if (!server.running) return { tone: "text-text-muted", text: `중지됨 · ${server.binary}` };
  return {
    tone: "text-status-done",
    text: `실행 중 · 127.0.0.1:${server.port} · pid ${server.pid}`,
  };
};

/**
 * 음성 입력 설정. STT 프로바이더 교체가 여기 URL 한 칸으로 끝나는 것이 설계 0043 의 핵심이라,
 * 연결 테스트도 `/models` 가 아니라 실제 전사 경로를 때린다.
 */
export function VoiceSettingsCard() {
  const [settings, setSettings] = useState<VoiceSettings | null>(null);
  const [status, setStatus] = useState<Status>({ kind: "idle", message: "" });
  const [busy, setBusy] = useState(false);
  const [server, setServer] = useState<VoiceServerStatus | null>(null);

  useEffect(() => {
    voiceSettingsGet()
      .then(setSettings)
      .catch((e) => setStatus({ kind: "error", message: String(e) }));
  }, []);

  // 서버를 앱이 띄우되 자동 spawn 은 하지 않는다 — 사용자 클릭으로만(ADR 0103 보완).
  // 그래서 여기서는 상태만 주기적으로 읽는다.
  useEffect(() => {
    let cancelled = false;
    const poll = () =>
      voiceServerStatus().then(
        (next) => !cancelled && setServer(next),
        // 실패는 상태 줄이 대신 말한다 — 폴링마다 status 를 덮어쓰지 않는다.
        () => !cancelled && setServer(null),
      );
    void poll();
    const timer = window.setInterval(() => void poll(), SERVER_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  if (!settings) return <div className="text-text-muted text-xs">불러오는 중…</div>;

  const update = (key: keyof VoiceSettings, value: string) =>
    setSettings({ ...settings, [key]: value });

  const save = async () => {
    setBusy(true);
    try {
      await voiceSettingsSet(settings);
      setStatus({ kind: "ok", message: "저장했습니다" });
    } catch (e) {
      // 저장은 됐지만 핫키 등록이 실패했을 수 있다 — 문구가 그 사실을 그대로 전달한다.
      setStatus({ kind: "error", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const test = async () => {
    setBusy(true);
    setStatus({ kind: "idle", message: "확인 중…" });
    try {
      await voiceSttTest(settings);
      setStatus({ kind: "ok", message: "연결 성공 — 전사 경로가 응답합니다" });
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  // 저장까지 하지 않는다 — 채워진 값을 보고 사용자가 확정한다.
  const applyPreset = () => {
    setSettings({ ...settings, ...OHR_PRESET });
    setStatus({ kind: "idle", message: "프리셋을 채웠습니다 — 저장을 눌러 확정하세요" });
  };

  const startServer = async () => {
    setBusy(true);
    try {
      // 저장된 값이 아니라 화면의 현재 값으로 띄운다(연결 테스트와 같은 규칙).
      setServer(await voiceServerStart(settings));
      setStatus({ kind: "ok", message: "서버 실행 중 — 연결 테스트로 확인하세요" });
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const stopServer = async () => {
    setBusy(true);
    try {
      setServer(await voiceServerStop());
      setStatus({ kind: "idle", message: "서버를 중지했습니다" });
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    } finally {
      setBusy(false);
    }
  };

  // 조회 전·조회 실패도 한 줄로 말한다 — 줄이 사라지면 잠긴 시작 버튼의 이유가 없어진다.
  const line = server
    ? serverLine(server)
    : { tone: "text-text-muted", text: "상태를 읽지 못했습니다" };

  return (
    <div className="space-y-2">
      <div className="space-y-2 border-b border-border pb-2 mb-2">
        <div className="text-text-secondary text-xs">로컬 서버 (ohr)</div>
        <div className={`text-[11px] ${line.tone}`}>{line.text}</div>
        <div className="flex items-center gap-2">
          <button className={BUTTON_CLASS} disabled={busy} onClick={applyPreset}>
            ohr 프리셋 적용
          </button>
          <button
            className={BUTTON_CLASS}
            disabled={busy || !server?.installed}
            onClick={() => void (server?.running ? stopServer() : startServer())}
          >
            {server?.running ? "서버 중지" : "서버 시작"}
          </button>
        </div>
      </div>

      {FIELDS.map((field) => (
        <label key={field.key} className="block">
          <div className="text-text-secondary text-xs">{field.label}</div>
          {field.hotkey ? (
            <HotkeyInput
              value={settings[field.key]}
              onChange={(spec) => update(field.key, spec)}
              conflicts={conflictsFor(field.key, settings)}
            />
          ) : (
            <input
              className="w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text outline-none focus:border-primary"
              type={field.type ?? "text"}
              value={settings[field.key]}
              spellCheck={false}
              onChange={(e) => update(field.key, e.target.value)}
            />
          )}
          {field.hint && <div className="text-text-muted text-[11px] mt-0.5">{field.hint}</div>}
        </label>
      ))}

      <div className="flex items-center gap-2 pt-1">
        <button className={BUTTON_CLASS} disabled={busy} onClick={() => void save()}>
          저장
        </button>
        <button className={BUTTON_CLASS} disabled={busy} onClick={() => void test()}>
          연결 테스트
        </button>
        {status.message && (
          <span
            className={`text-xs ${
              status.kind === "error"
                ? "text-status-failed"
                : status.kind === "ok"
                  ? "text-status-done"
                  : "text-text-muted"
            }`}
          >
            {status.message}
          </span>
        )}
      </div>
    </div>
  );
}
