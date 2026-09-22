//! PTY 코어 — Tauri 비의존. `cargo test`로 직접 검증 가능.
//!
//! `spawn()`이 `(PtySession, Receiver<PtyEvent>)`를 반환한다.
//! - reader 스레드: master 출력 → `PtyEvent::Output`
//! - waiter 스레드: 자식 종료 → `PtyEvent::Exit(code)`
//! - `terminate()`: process group 단위 SIGTERM → (대기) → SIGKILL (고아 방지)

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

/// PTY 세션당 스크롤백 링버퍼 기본 상한(2MiB) — attach 시 replay에 사용.
/// 디스크에는 절대 쓰지 않는다(메모리 전용 — 시크릿 잔존 방지, plans/0020 DR-6).
pub const DEFAULT_SCROLLBACK_CAP: usize = 2 * 1024 * 1024;

/// PTY 출력 스크롤백 — 상한 초과 시 앞부분을 제거하되 UTF-8 문자 경계를 넘지 않는다
/// (다중바이트 문자를 중간에서 절단하지 않음).
pub struct ScrollbackBuffer {
    data: VecDeque<u8>,
    cap: usize,
}

impl ScrollbackBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            data: VecDeque::new(),
            cap,
        }
    }

    /// 출력 청크를 뒤에 추가하고, 상한을 넘으면 앞부분을 잘라낸다.
    pub fn push(&mut self, chunk: &[u8]) {
        self.data.extend(chunk.iter().copied());
        self.truncate_to_cap();
    }

    /// 상한 초과분을 앞에서 제거한다. 절단 지점이 UTF-8 continuation byte(0b10xxxxxx)
    /// 중간이면 다음 문자 시작 바이트까지 전진시켜 잘린 결과가 항상 유효한 UTF-8이 되게 한다.
    fn truncate_to_cap(&mut self) {
        if self.data.len() <= self.cap {
            return;
        }
        let mut drop_to = self.data.len() - self.cap;
        while drop_to < self.data.len() && is_utf8_continuation(self.data[drop_to]) {
            drop_to += 1;
        }
        self.data.drain(0..drop_to);
    }

    /// 현재 버퍼 내용의 스냅샷(복사) — attach replay 페이로드로 쓴다.
    pub fn snapshot(&self) -> Vec<u8> {
        self.data.iter().copied().collect()
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

/// UTF-8이 어느 유닉스에나 설치돼 있는 최후 수단.
const FALLBACK_LOCALE: &str = "en_US.UTF-8";

/// 자식이 받아야 할 최소 터미널 환경을 채운다.
///
/// GUI(Finder·Launchpad)로 띄운 앱은 로그인 셸의 환경을 물려받지 못한다 — `LANG`도 `TERM`도
/// 비어 있다(`ps -Eww`로 확인 가능). 그대로 PTY에 넘기면 자식 셸이 C 로케일로 떨어져 UTF-8
/// 멀티바이트 입력이 바이트 단위로 쪼개진다: 한글 한 글자(3바이트)가 zsh 화면에 `<0085>`
/// 여러 개로 찍히고, 실행하면 `command not found: \M-^E\M-^D`가 뜬다. `TERM`이 없으면
/// terminfo 조회가 실패해 ZLE의 커서 이동·줄 갱신이 어긋나 입력한 글자가 중복돼 보인다.
///
/// 이미 값이 있으면 사용자 의도이므로 건드리지 않는다.
fn apply_terminal_env(builder: &mut CommandBuilder) {
    if env_value("TERM").is_none() {
        // 프론트가 xterm.js이므로 terminfo 이름을 거기에 맞춘다.
        builder.env("TERM", "xterm-256color");
    }
    if !locale_is_utf8(env_value) {
        builder.env("LANG", preferred_utf8_locale());
    }
}

/// 빈 문자열은 미설정과 같게 본다 — `LANG=`는 로케일을 지정하지 않는다.
fn env_value(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// POSIX 우선순위(LC_ALL > LC_CTYPE > LANG)로 문자 인코딩 로케일을 판정한다.
/// 상위 변수가 잡혀 있으면 하위는 보지 않는다 — `LC_ALL=C`는 `LANG=ko_KR.UTF-8`을 덮는다.
fn locale_is_utf8(lookup: impl Fn(&str) -> Option<String>) -> bool {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        let Some(value) = lookup(key) else { continue };
        let value = value.to_ascii_lowercase();
        return value.contains("utf-8") || value.contains("utf8");
    }
    false
}

/// 시스템 지역 설정에서 UTF-8 로케일 이름을 얻는다(프로세스당 1회 조회 후 캐시).
fn preferred_utf8_locale() -> String {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            system_locale()
                .as_deref()
                .and_then(utf8_locale_name)
                .unwrap_or_else(|| FALLBACK_LOCALE.to_string())
        })
        .clone()
}

#[cfg(target_os = "macos")]
fn system_locale() -> Option<String> {
    let out = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(not(target_os = "macos"))]
fn system_locale() -> Option<String> {
    None
}

/// macOS `AppleLocale`(`ko_KR`, `ko_KR@calendar=gregorian`)을 `.UTF-8` 로케일 이름으로 바꾼다.
/// `zh_Hans_CN`처럼 스크립트 서브태그가 낀 값은 대응하는 UTF-8 로케일이 설치돼 있지 않아,
/// 그대로 지정하면 셸이 `setlocale: cannot change locale` 경고를 낸다 — 채택하지 않고
/// 폴백에 맡긴다.
fn utf8_locale_name(apple_locale: &str) -> Option<String> {
    let base = apple_locale.split('@').next()?.trim();
    let (language, region) = base.split_once('_')?;
    let is_two_letters =
        |s: &str| s.len() == 2 && s.chars().all(|c: char| c.is_ascii_alphabetic());
    (is_two_letters(language) && is_two_letters(region)).then(|| format!("{base}.UTF-8"))
}

/// PTY 세션에서 발생하는 이벤트.
pub enum PtyEvent {
    /// 자식 프로세스의 stdout/stderr 바이트 청크.
    Output(Vec<u8>),
    /// 자식 종료 코드.
    Exit(i32),
}

/// webview emit 합치기 창 — emit 사이의 최소 간격. 이 간격 안에 도착한 청크는 다음 emit에 묶인다.
pub const EMIT_BATCH_WINDOW: Duration = Duration::from_millis(16);

/// 한 번의 emit이 실어 나르는 바이트 상한. 창이 아직 남았어도 여기서 끊는다.
pub const EMIT_BATCH_MAX_BYTES: usize = 64 * 1024;

/// 이미 큐에 있는 것만 훑은 결과.
enum Drained {
    /// 채널이 비었다 — 창이 남았으면 더 기다릴 수 있다.
    Empty,
    /// 더 기다리지 않고 즉시 돌려준다(종료 코드·채널 끊김·상한 도달).
    Done(Option<i32>),
}

/// 대기 없이 큐에 쌓인 것만 `buffer`로 옮긴다. 상한에 닿으면 거기서 멈춘다.
fn drain_ready(rx: &Receiver<PtyEvent>, buffer: &mut Vec<u8>) -> Drained {
    while buffer.len() < EMIT_BATCH_MAX_BYTES {
        match rx.try_recv() {
            Ok(PtyEvent::Output(more)) => buffer.extend_from_slice(&more),
            Ok(PtyEvent::Exit(code)) => return Drained::Done(Some(code)),
            Err(TryRecvError::Empty) => return Drained::Empty,
            Err(TryRecvError::Disconnected) => return Drained::Done(None),
        }
    }
    Drained::Done(None)
}

/// 전달 루프 하나당 하나. emit을 **선행**시키고 창은 **뒤에** 둔다 — 한적할 때의 첫 청크(키 에코)는
/// 기다림 없이 즉시 나가고, 창은 그다음 emit까지의 최소 간격으로만 쓰인다. 창을 먼저 기다리면
/// 낱개 키 입력마다 16ms가 그대로 얹힌다.
#[derive(Default)]
pub struct OutputCoalescer {
    last_emit: Option<Instant>,
}

impl OutputCoalescer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 직전 emit으로부터 창이 아직 안 닫혔으면 그 끝까지, 아니면 즉시(=대기 없음).
    fn deadline(&self, now: Instant) -> Instant {
        match self.last_emit {
            Some(last) if last + EMIT_BATCH_WINDOW > now => last + EMIT_BATCH_WINDOW,
            _ => now,
        }
    }

    /// 첫 청크에 후속 청크를 모아 한 덩어리로 돌려준다 — 전달 루프가 청크당 emit 대신
    /// 창당 emit 하게 한다. 도중 `Exit`를 만나면 모은 바이트와 함께 종료 코드를 돌려주고
    /// (호출자가 기존 Exit 분기로 보낸다), 채널이 끊기면 모은 것만 돌려준다.
    /// 바이트는 원본 그대로 — 손실 변환 없음.
    pub fn gather(&mut self, rx: &Receiver<PtyEvent>, first: Vec<u8>) -> (Vec<u8>, Option<i32>) {
        let deadline = self.deadline(Instant::now());
        let mut buffer = first;
        let exit = loop {
            if let Drained::Done(exit) = drain_ready(rx, &mut buffer) {
                break exit;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break None;
            }
            match rx.recv_timeout(remaining) {
                Ok(PtyEvent::Output(more)) => buffer.extend_from_slice(&more),
                Ok(PtyEvent::Exit(code)) => break Some(code),
                Err(_) => break None,
            }
        };
        self.last_emit = Some(Instant::now());
        (buffer, exit)
    }
}

/// 단일 PTY 세션 핸들. 살아있는 동안 master를 유지한다.
pub struct PtySession {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Box<dyn MasterPty + Send>,
    pid: u32,
    process_identity: Option<String>,
    /// 출력 스크롤백(메모리 전용) — attach 시 라이브 스트림 구독 전에 선전송(replay)한다.
    scrollback: Arc<Mutex<ScrollbackBuffer>>,
}

impl PtySession {
    /// `cmd`를 PTY에서 실행하고, 출력/종료 이벤트 수신용 Receiver와 함께 반환한다.
    pub fn spawn(
        cmd: &str,
        args: &[&str],
        cwd: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<(Self, Receiver<PtyEvent>)> {
        Self::spawn_with_env(cmd, args, cwd, cols, rows, &[])
    }

    /// `env`를 자식에 추가로 실어 실행한다(프리뷰 MCP Bearer 토큰 등 argv에 못 싣는 값).
    pub fn spawn_with_env(
        cmd: &str,
        args: &[&str],
        cwd: Option<&str>,
        cols: u16,
        rows: u16,
        env: &[(String, String)],
    ) -> anyhow::Result<(Self, Receiver<PtyEvent>)> {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut builder = CommandBuilder::new(cmd);
        builder.args(args);
        if let Some(dir) = cwd {
            builder.cwd(dir);
        }
        apply_terminal_env(&mut builder);
        for (key, value) in env {
            builder.env(key, value);
        }

        let mut child = pair.slave.spawn_command(builder)?;
        // PID 획득 실패는 즉시 에러 — terminate에서 killpg(0)로 자기 그룹을 죽이는 사고를 차단.
        let pid = child
            .process_id()
            .ok_or_else(|| anyhow::anyhow!("PTY 자식 PID 획득 실패"))?;
        let process_identity = crate::runner::process_identity::observe(pid).ok().flatten();

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        // 슬레이브 핸들을 닫아야 자식 종료 시 reader가 EOF를 받는다.
        drop(pair.slave);

        let (tx, rx) = channel::<PtyEvent>();
        let scrollback = Arc::new(Mutex::new(ScrollbackBuffer::new(DEFAULT_SCROLLBACK_CAP)));

        // reader 스레드: master → 스크롤백 기록 → Output(fan-out은 이 채널의 소비자가 담당 —
        // 작업 PTY·워크스페이스 셸·Runner terminal task가 모두 이 지점을 공유한다).
        let tx_out = tx.clone();
        let reader_scrollback = scrollback.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        reader_scrollback.lock().unwrap().push(chunk);
                        if tx_out.send(PtyEvent::Output(chunk.to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        // waiter 스레드: child.wait() → Exit
        std::thread::spawn(move || {
            let code = child
                .wait()
                .map(|s| i32::try_from(s.exit_code()).unwrap_or(-1))
                .unwrap_or(-1);
            let _ = tx.send(PtyEvent::Exit(code));
        });

        Ok((
            Self {
                writer: Mutex::new(writer),
                master: pair.master,
                pid,
                process_identity,
                scrollback,
            },
            rx,
        ))
    }

    /// 현재까지의 스크롤백 스냅샷(base64 인코딩 전) — attach 시 라이브 스트림 구독보다
    /// 먼저 호출해 replay 페이로드로 선전송한다.
    pub fn scrollback_snapshot(&self) -> Vec<u8> {
        self.scrollback.lock().unwrap().snapshot()
    }

    /// stdin으로 바이트 전달.
    pub fn write(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(data)?;
        w.flush()?;
        Ok(())
    }

    /// 터미널 크기 변경.
    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// 이 셸이 프롬프트에서 놀고 있는지 — 전경 프로세스 그룹이 셸 자신이면 참.
    ///
    /// 유휴 회수(ADR 0163 결정 4)가 이 판정에 기댄다. 마지막 I/O 시각으로 대신하면 두 방향으로
    /// 틀린다 — `tail -f`가 도는 셸은 출력이 계속 나와 영원히 회수되지 않고, vim을 열어둔 채
    /// 방치한 셸은 출력이 없어 회수 대상이 되어 편집 중이던 내용이 날아간다.
    ///
    /// `spawn_command`가 `setsid`로 자식을 세션 리더로 만들므로 pgid == pid다.
    /// `tcgetpgrp` 대응물이 없는 플랫폼에서는 판정하지 않는다(`None`) — 잘못된 근사로 남의
    /// 작업을 죽이는 것보다 셸이 남는 편이 낫다.
    pub fn at_prompt(&self) -> Option<bool> {
        #[cfg(unix)]
        {
            let leader = self.master.process_group_leader()?;
            Some(u32::try_from(leader).ok()? == self.pid)
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    pub fn process_identity(&self) -> Option<&str> {
        self.process_identity.as_deref()
    }

    /// 프로세스 그룹 단위로 SIGTERM → (800ms) → SIGKILL. 고아 자식 방지.
    ///
    /// 즉시 반환한다(시그널 시퀀스는 분리 스레드에서 수행) — 이벤트 루프/IPC 스레드
    /// (on_window_event, pty_kill, 재spawn)를 800ms 블로킹하지 않기 위함.
    pub fn terminate(&self) {
        let pid = self.pid;
        // pid==0 가드: killpg(0, ...)은 호출자 자신의 프로세스 그룹을 죽인다.
        if pid == 0 {
            return;
        }
        #[cfg(unix)]
        {
            std::thread::spawn(move || {
                use nix::sys::signal::{killpg, Signal};
                use nix::unistd::Pid;
                let pgid = Pid::from_raw(pid as i32);
                let _ = killpg(pgid, Signal::SIGTERM);
                std::thread::sleep(std::time::Duration::from_millis(800));
                let _ = killpg(pgid, Signal::SIGKILL);
            });
        }
        #[cfg(windows)]
        {
            // Windows: 프로세스 트리 강제 종료(taskkill /T = 자식 포함, /F = force).
            std::thread::spawn(move || {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .output();
            });
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = pid;
        }
    }
}

#[cfg(test)]
mod scrollback_tests {
    use super::*;

    #[test]
    fn keeps_all_bytes_under_cap() {
        let mut buf = ScrollbackBuffer::new(1024);
        buf.push(b"hello");
        buf.push(b" world");
        assert_eq!(buf.snapshot(), b"hello world");
    }

    #[test]
    fn truncates_from_front_when_over_cap() {
        let mut buf = ScrollbackBuffer::new(10);
        buf.push(b"0123456789"); // 정확히 10바이트
        buf.push(b"AB"); // 12바이트 → 앞 2바이트 제거
        let snap = buf.snapshot();
        assert!(snap.len() <= 10);
        assert_eq!(snap, b"23456789AB");
    }

    #[test]
    fn cap_enforced_across_many_pushes() {
        let mut buf = ScrollbackBuffer::new(100);
        for _ in 0..50 {
            buf.push(b"0123456789");
        }
        assert!(buf.snapshot().len() <= 100);
    }

    /// 절단 지점이 다중바이트 문자의 리드 바이트(continuation 아님)와 우연히 겹치는 경우.
    #[test]
    fn truncation_at_lead_byte_stays_utf8_valid() {
        let mut buf = ScrollbackBuffer::new(4);
        buf.push("a€b".as_bytes()); // a(1) + €(3, E2 82 AC) + b(1) = 5바이트
        let snap = buf.snapshot();
        assert!(snap.len() <= 4);
        assert!(
            String::from_utf8(snap.clone()).is_ok(),
            "잘린 결과가 유효한 UTF-8이어야 함: {snap:?}"
        );
    }

    /// 절단 지점이 다중바이트 문자의 continuation byte 중간에 걸리는 경우 —
    /// 다음 문자 시작까지 전진해야 한다(중간 절단 금지).
    #[test]
    fn truncation_skips_past_continuation_bytes_mid_character() {
        let mut buf = ScrollbackBuffer::new(3);
        buf.push("a€c".as_bytes()); // a(0x61) + E2 82 AC + c(0x63) = 5바이트, drop_to=2는 0x82(continuation)
        let snap = buf.snapshot();
        assert!(snap.len() <= 3);
        assert!(
            String::from_utf8(snap.clone()).is_ok(),
            "다중바이트 문자 중간 절단 없이 유효한 UTF-8이어야 함: {snap:?}"
        );
        assert_eq!(snap, b"c");
    }

    #[test]
    fn empty_buffer_snapshot_is_empty() {
        let buf = ScrollbackBuffer::new(1024);
        assert!(buf.snapshot().is_empty());
    }

    /// GUI 실행 환경 — 로케일 변수가 하나도 없으면 UTF-8이 아니라고 봐야 한다.
    #[test]
    fn empty_env_is_not_utf8_locale() {
        assert!(!locale_is_utf8(|_| None));
        assert!(!locale_is_utf8(|_| Some(String::new())));
        assert!(!locale_is_utf8(|_| Some("C".to_string())));
    }

    #[test]
    fn utf8_locale_detected_from_any_posix_variable() {
        for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
            assert!(
                locale_is_utf8(|k| (k == key).then(|| "ko_KR.UTF-8".to_string())),
                "{key}로 지정한 UTF-8 로케일을 인식해야 함"
            );
        }
        assert!(locale_is_utf8(|_| Some("en_US.utf8".to_string())));
    }

    /// 상위 변수가 잡혀 있으면 하위는 보지 않는다 — `LC_ALL=C`가 `LANG=…UTF-8`을 덮는다.
    #[test]
    fn higher_priority_locale_variable_wins() {
        let lookup = |k: &str| match k {
            "LC_ALL" => Some("C".to_string()),
            "LANG" => Some("ko_KR.UTF-8".to_string()),
            _ => None,
        };
        assert!(!locale_is_utf8(lookup));
    }

    #[test]
    fn apple_locale_becomes_utf8_locale_name() {
        assert_eq!(utf8_locale_name("ko_KR").as_deref(), Some("ko_KR.UTF-8"));
        assert_eq!(
            utf8_locale_name("ko_KR@calendar=gregorian").as_deref(),
            Some("ko_KR.UTF-8")
        );
    }

    /// 스크립트 서브태그·언어 단독 값은 대응 UTF-8 로케일이 없으므로 폴백에 맡긴다.
    #[test]
    fn apple_locale_without_plain_language_region_is_rejected() {
        assert_eq!(utf8_locale_name("zh_Hans_CN"), None);
        assert_eq!(utf8_locale_name("en"), None);
        assert_eq!(utf8_locale_name(""), None);
    }

    #[test]
    fn fallback_locale_is_utf8() {
        assert!(locale_is_utf8(|_| Some(FALLBACK_LOCALE.to_string())));
        assert!(locale_is_utf8(|_| Some(preferred_utf8_locale())));
    }

    /// 회귀 방지의 핵심 — 자식 셸은 UTF-8 로케일과 terminfo 이름을 반드시 받아야 한다.
    /// 이게 비면 한글 입력이 바이트로 쪼개져 `command not found: \M-^E`가 뜬다.
    #[test]
    fn spawned_child_gets_utf8_locale_and_term() {
        let (session, rx) = PtySession::spawn(
            "/bin/sh",
            // 실효 로케일은 POSIX 우선순위로 뽑는다(LC_ALL > LC_CTYPE > LANG).
            &[
                "-c",
                r#"printf 'TERM=[%s] LOCALE=[%s]' "$TERM" "${LC_ALL:-${LC_CTYPE:-$LANG}}""#,
            ],
            None,
            80,
            24,
        )
        .expect("PTY spawn");

        let mut out = Vec::new();
        while let Ok(ev) = rx.recv_timeout(std::time::Duration::from_secs(10)) {
            match ev {
                PtyEvent::Output(chunk) => out.extend_from_slice(&chunk),
                PtyEvent::Exit(_) => break,
            }
        }
        session.terminate();

        let text = String::from_utf8_lossy(&out).to_ascii_lowercase();
        assert!(
            !text.contains("term=[]"),
            "자식에 TERM이 비어 있으면 안 됨: {text}"
        );
        assert!(
            text.contains("utf-8") || text.contains("utf8"),
            "자식의 실효 로케일이 UTF-8이어야 함: {text}"
        );
    }
}

#[cfg(test)]
mod coalesce_tests {
    use super::*;

    #[test]
    fn gathers_pending_chunks_in_order() {
        let (tx, rx) = channel::<PtyEvent>();
        for i in 0..5u8 {
            tx.send(PtyEvent::Output(vec![b'a' + i])).unwrap();
        }
        let (bytes, exit) = OutputCoalescer::new().gather(&rx, Vec::new());
        assert_eq!(bytes, b"abcde");
        assert!(exit.is_none());
    }

    #[test]
    fn stops_at_cap_and_leaves_rest_in_channel() {
        let (tx, rx) = channel::<PtyEvent>();
        tx.send(PtyEvent::Output(vec![b'x'; EMIT_BATCH_MAX_BYTES]))
            .unwrap();
        tx.send(PtyEvent::Output(b"tail".to_vec())).unwrap();
        let (bytes, exit) = OutputCoalescer::new().gather(&rx, Vec::new());
        assert_eq!(bytes.len(), EMIT_BATCH_MAX_BYTES);
        assert!(exit.is_none());
        match rx.recv().unwrap() {
            PtyEvent::Output(rest) => assert_eq!(rest, b"tail"),
            PtyEvent::Exit(code) => panic!("남은 이벤트가 Output이어야 함: Exit({code})"),
        }
    }

    #[test]
    fn returns_exit_code_with_gathered_bytes() {
        let (tx, rx) = channel::<PtyEvent>();
        tx.send(PtyEvent::Output(b"bye".to_vec())).unwrap();
        tx.send(PtyEvent::Exit(3)).unwrap();
        let (bytes, exit) = OutputCoalescer::new().gather(&rx, Vec::new());
        assert_eq!(bytes, b"bye");
        assert_eq!(exit, Some(3));
    }

    #[test]
    fn returns_gathered_bytes_when_sender_disconnects() {
        let (tx, rx) = channel::<PtyEvent>();
        tx.send(PtyEvent::Output(b"tail".to_vec())).unwrap();
        drop(tx);
        let (bytes, exit) = OutputCoalescer::new().gather(&rx, b"head".to_vec());
        assert_eq!(bytes, b"headtail");
        assert!(exit.is_none());
    }

    /// 키 에코 한 글자가 창만큼 늦지 않아야 한다 — 선행 emit의 핵심.
    #[test]
    fn idle_first_chunk_returns_without_waiting() {
        let (_tx, rx) = channel::<PtyEvent>();
        let started = Instant::now();
        let (bytes, exit) = OutputCoalescer::new().gather(&rx, b"k".to_vec());
        assert_eq!(bytes, b"k");
        assert!(exit.is_none());
        assert!(
            started.elapsed() < Duration::from_millis(5),
            "한적한 첫 청크는 즉시 반환되어야 함: {:?}",
            started.elapsed()
        );
    }

    /// 직전 emit 이후 창이 닫히기 전에 온 청크는 같은 덩어리로 묶인다.
    #[test]
    fn chunk_arriving_within_window_is_gathered_until_it_closes() {
        let (tx, rx) = channel::<PtyEvent>();
        let mut coalescer = OutputCoalescer::new();
        // 첫 emit이 `last_emit`을 세운다 — 이 시점부터 창이 열려 있다.
        let (first, _) = coalescer.gather(&rx, b"a".to_vec());
        assert_eq!(first, b"a");

        let sender = tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(3));
            let _ = sender.send(PtyEvent::Output(b"c".to_vec()));
        });
        let started = Instant::now();
        let (bytes, exit) = coalescer.gather(&rx, b"b".to_vec());
        assert_eq!(bytes, b"bc");
        assert!(exit.is_none());
        assert!(
            started.elapsed() >= Duration::from_millis(3),
            "창이 닫힐 때까지 모아야 함: {:?}",
            started.elapsed()
        );
    }
}
