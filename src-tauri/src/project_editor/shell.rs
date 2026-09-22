use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::registry::ProjectLaunch;
use crate::pty::{PtyEvent, PtySession, ScrollbackBuffer};

pub struct ProjectShell {
    pub session: u64,
    pty: PtySession,
    output: ScrollbackBuffer,
    sequence: u64,
    exited: bool,
    exit_code: Option<i32>,
}

#[derive(Serialize)]
pub struct ShellOpen {
    pub session: u64,
    pub existed: bool,
}

#[derive(Serialize)]
pub struct ShellSnapshot {
    pub session: u64,
    pub sequence: u64,
    pub data: String,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Serialize)]
struct ShellOutput {
    session: u64,
    sequence: u64,
    data: String,
}

#[derive(Clone, Serialize)]
struct ShellExit {
    session: u64,
    code: i32,
}

impl ProjectShell {
    /// `launch`가 있으면 그 명령을, 없으면 사용자 기본 셸을 창의 PTY로 띄운다.
    pub fn spawn(
        root: &str,
        session: u64,
        cols: u16,
        rows: u16,
        launch: Option<&ProjectLaunch>,
    ) -> Result<(Arc<Mutex<Self>>, std::sync::mpsc::Receiver<PtyEvent>), String> {
        let (cmd, args) = match launch {
            Some(launch) => (launch.bin.clone(), launch.args.clone()),
            None => {
                let spec = crate::commands::default_shell();
                (spec.cmd, spec.args)
            }
        };
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (pty, events) = PtySession::spawn(&cmd, &args, Some(root), cols, rows)
            .map_err(|error| format!("셸 생성 실패: {error}"))?;
        Ok((
            Arc::new(Mutex::new(Self {
                session,
                pty,
                output: ScrollbackBuffer::new(crate::pty::DEFAULT_SCROLLBACK_CAP),
                sequence: 0,
                exited: false,
                exit_code: None,
            })),
            events,
        ))
    }

    pub fn snapshot(&self, session: u64) -> Result<ShellSnapshot, String> {
        self.check_session(session)?;
        Ok(ShellSnapshot {
            session,
            sequence: self.sequence,
            data: STANDARD.encode(self.output.snapshot()),
            exited: self.exited,
            exit_code: self.exit_code,
        })
    }

    pub fn write(&self, session: u64, data: &str) -> Result<(), String> {
        self.check_session(session)?;
        self.pty
            .write(data.as_bytes())
            .map_err(|error| error.to_string())
    }

    pub fn resize(&self, session: u64, cols: u16, rows: u16) -> Result<(), String> {
        self.check_session(session)?;
        self.pty
            .resize(cols, rows)
            .map_err(|error| error.to_string())
    }

    pub fn terminate(&self) {
        self.pty.terminate();
    }

    pub fn close(&mut self, session: u64) -> Result<(), String> {
        self.check_session(session)?;
        self.exited = true;
        self.pty.terminate();
        Ok(())
    }

    pub fn exited(&self) -> bool {
        self.exited
    }

    pub(crate) fn check_session(&self, session: u64) -> Result<(), String> {
        if is_current_session(self.session, session) {
            Ok(())
        } else {
            Err("오래된 프로젝트 셸 세션입니다".into())
        }
    }
}

fn is_current_session(current: u64, requested: u64) -> bool {
    current == requested
}

pub fn forward_events<R: tauri::Runtime>(
    app: AppHandle<R>,
    label: String,
    shell: Arc<Mutex<ProjectShell>>,
    events: std::sync::mpsc::Receiver<PtyEvent>,
) {
    std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            match event {
                PtyEvent::Output(data) => emit_output(&app, &label, &shell, data),
                PtyEvent::Exit(code) => emit_exit(&app, &label, &shell, code),
            }
        }
    });
}

fn emit_output<R: tauri::Runtime>(
    app: &AppHandle<R>,
    label: &str,
    shell: &Arc<Mutex<ProjectShell>>,
    data: Vec<u8>,
) {
    let payload = {
        let mut shell = shell.lock().unwrap();
        shell.sequence += 1;
        shell.output.push(&data);
        ShellOutput {
            session: shell.session,
            sequence: shell.sequence,
            data: STANDARD.encode(data),
        }
    };
    let _ = app.emit_to(label, "project-shell://output", payload);
}

fn emit_exit<R: tauri::Runtime>(
    app: &AppHandle<R>,
    label: &str,
    shell: &Arc<Mutex<ProjectShell>>,
    code: i32,
) {
    let session = {
        let mut shell = shell.lock().unwrap();
        shell.exited = true;
        shell.exit_code = Some(code);
        shell.session
    };
    let _ = app.emit_to(label, "project-shell://exit", ShellExit { session, code });
}
