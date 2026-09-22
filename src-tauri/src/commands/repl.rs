//! 작업별 Python REPL(IPython) PTY 채널 — `shell.rs`와 나란히, 별도 맵·이벤트로 존재한다.
//!
//! `shell_*`가 워크트리에서 여는 인터랙티브 셸과 달리 이쪽은 **IPython 프로세스 하나**를
//! 키핑한다. 유휴 회수(`shellreap::reap`) 대상이 아니다 — REPL의 가치는 살아 있는 인터프리터
//! 메모리 상태(변수·import) 그 자체라서, 셸처럼 "안 보면 죽인다"를 적용하면 그 상태가 날아간다.

use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db;
use crate::pty::{OutputCoalescer, PtyEvent, PtySession};
use crate::repl_launch::{self, PromptScanner, ReplLaunch};
use crate::shellreap::ShellSlot;

use super::{AppState, ExitPayload, OutputPayload, pool_of};

/// `repl_open`의 결과 — 프론트가 `status`로 분기한다.
#[derive(serde::Serialize)]
pub struct ReplOpenResult {
    /// "existed" | "started" | "missing".
    pub status: String,
    /// "missing"일 때만 값을 가진다 — 설치에 쓸 python3 절대경로(있으면).
    pub python: Option<String>,
}

/// Python REPL PTY 슬롯. attach/detach·부착 카운트는 `ShellSlot`을 그대로 감싼다 — REPL도
/// 도크와 탭이 같은 프로세스를 볼 수 있다는 점은 워크스페이스 셸과 동일하기 때문이다.
/// 다만 이 슬롯은 `shellreap::reap`이 도는 맵(`AppState.shells`)에 들어가지 않으므로
/// 유휴 시간과 무관하게 살아 있다 — 그것이 REPL을 만든 이유다.
pub struct ReplSlot {
    pub slot: ShellSlot,
    /// IPython 프롬프트(`In [`)를 아직 못 봤으면 false. 그동안 들어온 코드는 `pending`에 쌓인다.
    pub ready: bool,
    /// `ready`가 되기 전에 `repl_run`이 넣은 코드 — ready 전이 시점에 도착 순서대로 흘려보낸다.
    pub pending: Vec<String>,
}

impl ReplSlot {
    fn new(session: PtySession) -> Self {
        Self {
            slot: ShellSlot::new(session),
            ready: false,
            pending: Vec::new(),
        }
    }
}

/// 출력 청크를 스캐너에 먹이고, 이번 호출에서 처음 프롬프트가 보이면 밀린 `pending`을
/// 도착 순서대로 흘려보낸다. reader 스레드와 PTY 통합 테스트가 공유하는 지점 —
/// 진짜 PTY를 붙여도 이 함수 하나만 부르면 같은 결과가 나온다는 뜻이다.
fn on_output(slot: &mut ReplSlot, scanner: &mut PromptScanner, chunk: &[u8]) {
    if slot.ready {
        return;
    }
    if scanner.feed(chunk) {
        slot.ready = true;
        for code in slot.pending.drain(..) {
            let _ = slot.slot.session.write(code.as_bytes());
        }
    }
}

/// Python REPL 열기 — 작업 워크트리에서 IPython을 띄운다(작업당 1개).
/// 이미 열려 있으면 재사용(`existed`). ipython이 없고 `install`이 false면 스폰하지 않고
/// `missing`으로 python3 경로를 돌려준다 — 프론트가 설치 여부를 물은 뒤 `install: true`로
/// 다시 부르는 것을 전제한다.
#[tauri::command]
pub async fn repl_open(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    cols: u16,
    rows: u16,
    install: bool,
) -> Result<ReplOpenResult, String> {
    if let Some(slot) = state.repls.lock().unwrap().get_mut(&id) {
        slot.slot.attach();
        return Ok(ReplOpenResult {
            status: "existed".into(),
            python: None,
        });
    }
    let pool = pool_of(&state)?;
    let task = db::get_task(&pool, id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("작업을 찾을 수 없습니다")?;
    if !Path::new(&task.worktree_path).is_dir() {
        return Err("워크트리가 없습니다 — 종료된 작업일 수 있습니다".into());
    }

    let launch = repl_launch::resolve(Path::new(&task.worktree_path));
    let (cmd, args): (String, Vec<String>) = match launch {
        ReplLaunch::Ipython { cmd, args } => (cmd, args),
        ReplLaunch::Missing { python } => {
            if !install {
                return Ok(ReplOpenResult {
                    status: "missing".into(),
                    python,
                });
            }
            match python {
                Some(py) => repl_launch::install_launch(&py),
                None => return Err("python3를 찾을 수 없습니다".into()),
            }
        }
    };

    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (session, rx) = PtySession::spawn(&cmd, &argv, Some(&task.worktree_path), cols, rows)
        .map_err(|e| format!("Python 콘솔 생성 실패: {e}"))?;
    let session_pid = session.pid();
    {
        // 삽입 직전 재확인(동시 open 레이스) — 진 쪽의 새 세션은 정리하고 기존 것을 쓴다.
        let mut repls = state.repls.lock().unwrap();
        if let Some(slot) = repls.get_mut(&id) {
            session.terminate();
            slot.slot.attach();
            return Ok(ReplOpenResult {
                status: "existed".into(),
                python: None,
            });
        }
        let mut slot = ReplSlot::new(session);
        slot.slot.attach();
        repls.insert(id, slot);
    }
    std::thread::spawn(move || {
        let output_event = format!("repl://output/{id}");
        let exit_event = format!("repl://exit/{id}");
        let mut coalescer = OutputCoalescer::new();
        let mut scanner = PromptScanner::new();
        while let Ok(ev) = rx.recv() {
            let code = match ev {
                PtyEvent::Output(b) => {
                    let (bytes, pending_exit) = coalescer.gather(&rx, b);
                    {
                        // 프롬프트 판정과 pending flush는 emit 이전에 — 프론트가 결과를 보기
                        // 전에 이미 큐가 흘러가 있어야 순서가 뒤섞이지 않는다.
                        let state = app.state::<AppState>();
                        let mut repls = state.repls.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(slot) = repls.get_mut(&id) {
                            on_output(slot, &mut scanner, &bytes);
                        }
                    }
                    let _ = app.emit(
                        &output_event,
                        OutputPayload {
                            id,
                            data: STANDARD.encode(&bytes),
                        },
                    );
                    match pending_exit {
                        Some(code) => code,
                        None => continue,
                    }
                }
                PtyEvent::Exit(code) => code,
            };
            // 죽은 슬롯이 맵에 남지 않도록 제거. 그 자리에 이미 다른 REPL이 들어와 있으면
            // 건드리지 않는다(늦은 종료 이벤트).
            let state = app.state::<AppState>();
            let mut repls = state.repls.lock().unwrap_or_else(|e| e.into_inner());
            if repls.get(&id).map(|s| s.slot.session.pid()) == Some(session_pid) {
                repls.remove(&id);
            }
            drop(repls);
            let _ = app.emit(&exit_event, ExitPayload { id, code });
            break;
        }
    });
    Ok(ReplOpenResult {
        status: "started".into(),
        python: None,
    })
}

/// 코드 실행 — bracketed paste로 감싸 IPython에 붙여넣는다. 프롬프트가 아직 안 보이면
/// (설치·기동 중) `pending`에 쌓아 두고, 프롬프트가 보이는 순간 순서대로 흘려보낸다.
#[tauri::command]
pub async fn repl_run(state: State<'_, AppState>, id: i64, code: String) -> Result<(), String> {
    if code.trim().is_empty() {
        return Ok(());
    }
    let payload = repl_launch::bracketed_paste(&code);
    let mut repls = state.repls.lock().unwrap_or_else(|e| e.into_inner());
    let slot = repls.get_mut(&id).ok_or("열린 Python 콘솔이 없습니다")?;
    if slot.ready {
        slot.slot
            .session
            .write(payload.as_bytes())
            .map_err(|e| e.to_string())
    } else {
        slot.pending.push(payload);
        Ok(())
    }
}

/// Python REPL stdin 직접 전달(키 입력 에코 등) — `repl_run`과 달리 bracketed paste로
/// 감싸지 않는다.
#[tauri::command]
pub async fn repl_write(state: State<'_, AppState>, id: i64, data: String) -> Result<(), String> {
    let repls = state.repls.lock().unwrap_or_else(|e| e.into_inner());
    let slot = repls.get(&id).ok_or("열린 Python 콘솔이 없습니다")?;
    slot.slot.session.write(data.as_bytes()).map_err(|e| e.to_string())
}

/// Python REPL 리사이즈.
#[tauri::command]
pub async fn repl_resize(
    state: State<'_, AppState>,
    id: i64,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let repls = state.repls.lock().unwrap_or_else(|e| e.into_inner());
    let slot = repls.get(&id).ok_or("열린 Python 콘솔이 없습니다")?;
    slot.slot.session.resize(cols, rows).map_err(|e| e.to_string())
}

/// Python REPL 스크롤백 replay(base64) — 열린 REPL이 없으면 빈 문자열(에러 아님).
///
/// 셸의 `shell_replay`와 달리 "유휴 회수" 안내 로직이 없다 — REPL은 애초에 회수되지 않는다.
#[tauri::command]
pub async fn repl_replay(state: State<'_, AppState>, id: i64) -> Result<String, String> {
    let repls = state.repls.lock().unwrap_or_else(|e| e.into_inner());
    let scrollback = repls.get(&id).map(|slot| slot.slot.session.scrollback_snapshot());
    Ok(scrollback.map(|b| STANDARD.encode(b)).unwrap_or_default())
}

/// Python REPL에서 화면이 떨어졌음을 알린다. REPL은 그대로 살아 있다 — 유휴 회수가 없으므로
/// `shell_detach`처럼 회수 판정의 재료로 쓰이지는 않지만, attach 카운트는 셸과 동일하게 갱신한다.
#[tauri::command]
pub fn repl_detach(state: State<AppState>, id: i64) -> Result<(), String> {
    if let Some(slot) = state.repls.lock().unwrap().get_mut(&id) {
        slot.slot.detach();
    }
    Ok(())
}

/// Python REPL 종료(명시적 닫기).
#[tauri::command]
pub fn repl_close(state: State<AppState>, id: i64) -> Result<(), String> {
    if let Some(slot) = state.repls.lock().unwrap().remove(&id) {
        slot.slot.session.terminate();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 프롬프트가 보이기 전에 큐잉된 코드는, 프롬프트가 보인 뒤에야 `cat`을 거쳐 되돌아온다.
    #[cfg(unix)]
    #[test]
    fn 프롬프트_전에_큐잉한_코드는_프롬프트_이후에만_흘러간다() {
        let (session, rx) = PtySession::spawn(
            "/bin/sh",
            &["-c", "printf 'In [1]: '; cat"],
            None,
            80,
            24,
        )
        .expect("PTY spawn");

        let mut slot = ReplSlot::new(session);
        let mut scanner = PromptScanner::new();

        // 프롬프트가 아직 안 보이는 시점에 코드를 큐잉한다 — 이 시점엔 아직 아무것도 못 봤다.
        assert!(!slot.ready);
        slot.pending.push("echo-me\n".to_string());

        // 프롬프트("In [")가 처음 보이는 순간 바로 멈춘다 — 그 전까지는 pending이 그대로다.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !slot.ready && Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(PtyEvent::Output(b)) => on_output(&mut slot, &mut scanner, &b),
                Ok(PtyEvent::Exit(_)) => break,
                Err(_) => continue,
            }
        }

        assert!(slot.ready, "프롬프트를 봤어야 함");
        assert!(slot.pending.is_empty(), "flush 후 pending은 비어야 함");

        // cat이 되돌려준 echo를 스크롤백에서 확인한다 — pending이 프롬프트 이후에만 나갔다는 증거.
        let mut got_echo = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let snap = slot.slot.session.scrollback_snapshot();
            if String::from_utf8_lossy(&snap).contains("echo-me") {
                got_echo = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        slot.slot.session.terminate();
        assert!(got_echo, "프롬프트 이후 flush된 pending이 cat을 거쳐 되돌아와야 함");
    }
}
