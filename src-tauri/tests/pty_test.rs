//! PTY 코어 통합 테스트 (Tauri 비의존).
//! 실행: `cargo test`

use std::time::Duration;

use praxis_lib::pty::{PtyEvent, PtySession};

/// R3: 출력 바이트가 스트리밍된다.
#[test]
fn echo_output_is_streamed() {
    let (session, rx) =
        PtySession::spawn("/bin/sh", &["-c", "echo praxis_marker"], None, 80, 24).expect("spawn");
    let mut seen = String::new();
    while let Ok(ev) = rx.recv_timeout(Duration::from_secs(2)) {
        match ev {
            PtyEvent::Output(b) => seen.push_str(&String::from_utf8_lossy(&b)),
            PtyEvent::Exit(_) => break,
        }
    }
    drop(session);
    assert!(seen.contains("praxis_marker"), "got: {seen:?}");
}

/// R5: stdin이 자식 프로세스로 전달된다.
#[test]
fn stdin_is_forwarded() {
    let (session, rx) = PtySession::spawn("/bin/cat", &[], None, 80, 24).expect("spawn");
    session.write(b"hello_pty\n").expect("write");
    let mut seen = String::new();
    while let Ok(ev) = rx.recv_timeout(Duration::from_secs(2)) {
        if let PtyEvent::Output(b) = ev {
            seen.push_str(&String::from_utf8_lossy(&b));
            if seen.contains("hello_pty") {
                break;
            }
        }
    }
    drop(session);
    assert!(seen.contains("hello_pty"), "got: {seen:?}");
}

/// R7: terminate()가 1초 내 프로세스를 종료한다.
#[test]
fn terminate_kills_within_one_second() {
    let (session, rx) =
        PtySession::spawn("/bin/sh", &["-c", "sleep 30"], None, 80, 24).expect("spawn");
    let start = std::time::Instant::now();
    session.terminate();
    let mut exited = false;
    while start.elapsed() < Duration::from_millis(1500) {
        if let Ok(PtyEvent::Exit(_)) = rx.recv_timeout(Duration::from_millis(200)) {
            exited = true;
            break;
        }
    }
    assert!(exited, "process did not exit after terminate()");
}
