//! 단발 실행 벤더(Claude)의 질문 세션. 질문은 인앱 MCP 툴 호출이 되고, 그 호출은 답이 올 때까지
//! **서버 쪽에서 멈춘다** — 그래서 턴 프로세스는 양방향 stdin을 열 필요가 없다.
//!
//! 세션 등록이 곧 툴의 존재다. 등록이 없으면 `ask_user`는 목록에도 없고 호출도 거절된다.

use super::app_server::Context;
use super::{interaction as ledger, ConvoEvent, TurnOutcome};
use crate::preview_bridge::mcp::PreviewMcpLease;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// 지시문은 메시지 앞에 붙는다. Claude 경로에는 `--append-system-prompt`가 이미 턴 계약을 쓰고
/// 있어, 여기에 얹으면 둘 중 하나가 밀린다.
pub const TOOL_INSTRUCTIONS: &str = "# Praxis clarification contract\n- To ask the user a question, call the MCP tool `mcp__praxis-preview__ask_user` with kind=clarification and 1-3 questions (id, question, options with id/label/description, allow_free_text, is_secret=false).\n- Ask only ordinary clarifications. Never ask for secrets, credentials, or tool execution approvals, and never treat an answer as a permission grant.\n- The tool blocks until the user replies and returns their answers. Do not ask the user anything in plain prose and then stop — a question asked outside this tool never reaches the user.\n- While it is pending, continue only work that does not depend on the reply.\n- Leave no question unanswered when the turn ends; the turn fails if one is still open.";

const POLL: Duration = Duration::from_millis(200);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// 등록된 턴. `anchors`는 질문이 트랜스크립트에 남길 이벤트의 통로다 — `ask`는 벤더 스트림을
/// 읽는 스레드가 아니라 MCP 핸들러(tokio)에서 돌고, 스트림 콜백은 그 스레드에 묶여 있다.
#[derive(Clone)]
struct Session {
    ctx: Context,
    anchors: Sender<ConvoEvent>,
}

fn sessions() -> &'static Mutex<HashMap<i64, Session>> {
    static MAP: OnceLock<Mutex<HashMap<i64, Session>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// 등록의 수명이 툴의 수명이다 — 턴이 어떻게 끝나든(패닉 포함) 여기서 거둬진다.
struct Registered(i64);
impl Drop for Registered {
    fn drop(&mut self) {
        sessions()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

/// 이 작업의 턴이 지금 질문을 받을 수 있는가. `tools/list`가 이것으로 갈린다.
pub fn active(task: i64) -> bool {
    sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&task)
}

fn session(task: i64) -> Option<Session> {
    sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&task)
        .cloned()
}

/// 밀린 질문 앵커를 벤더 이벤트 **앞에** 싣는다. 답을 실은 `tool_result`보다 먼저 자리를 잡아야
/// 그 뒤의 도구 카드가 질문 아래로 쌓인다 — Codex 경로가 `open` 직후 `on_event`하는 것과 같은 순서다.
fn drain_anchors(anchors: &Receiver<ConvoEvent>, on_event: &mut impl FnMut(ConvoEvent)) {
    while let Ok(anchor) = anchors.try_recv() {
        on_event(anchor);
    }
}

/// MCP 핸들러가 부르는 진입점. **`Context::db`를 쓰지 않는다** — 그것은 `block_on`이고
/// 여기는 이미 tokio 런타임 안이다.
pub async fn ask(task: i64, arguments: &Value) -> Result<String, String> {
    let Session { ctx, anchors } = session(task).ok_or("이 작업에는 열린 질문 세션이 없습니다")?;
    let execution = ctx.control.execution.clone();
    // 원장의 UNIQUE(execution, wire_id)를 지키려면 우리가 id를 만들어야 한다 — MCP JSON-RPC id는
    // 세션마다 1부터 되풀이된다. 응답은 같은 프로세스 안에서 돌려주므로 상관 id가 필요 없다.
    let wire = Value::String(ledger::id()?);
    let call = ledger::id()?;
    let interaction =
        ledger::open(&ctx.pool, &execution, &wire, &call, arguments, crate::now()).await?;
    // 트랜스크립트 앵커. 없으면 카드는 늘 대화 맨 아래 폴백에 그려져, 답한 뒤의 도구 카드가
    // 질문 **위로** 쌓인다. 수신 쪽이 이미 닫혔으면(턴 종료 경합) 앵커만 잃고 질문은 그대로다.
    let _ = anchors.send(ConvoEvent::Interaction {
        interaction_id: interaction.clone(),
    });
    ctx.changed();
    loop {
        if ctx.control.cancelled.load(Ordering::SeqCst) {
            return Err("사용자가 턴을 중단했습니다".into());
        }
        if let Some(answer) =
            ledger::take_dispatch_for(&ctx.pool, &execution, &interaction, crate::now()).await?
        {
            ledger::written(&ctx.pool, &answer.answer_id).await?;
            ledger::settle(&ctx.pool, &execution, &answer.call_id).await?;
            ctx.changed();
            return Ok(answer.output);
        }
        if !ledger::interaction_open(&ctx.pool, &interaction, crate::now()).await? {
            return Err("질문이 응답 없이 닫혔습니다".into());
        }
        tokio::time::sleep(POLL).await;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    ctx: &Context,
    cwd: &str,
    message: &str,
    resume: Option<&str>,
    idle_timeout: u64,
    vendor: super::Vendor,
    bin: &str,
    model: Option<&str>,
    effort: Option<&str>,
    service_tier: Option<&str>,
    images: &[String],
    session_name: Option<&str>,
    mcp: Option<&PreviewMcpLease>,
    on_spawn: impl FnOnce(u32),
    mut on_event: impl FnMut(ConvoEvent),
) -> Result<TurnOutcome, String> {
    let execution = ctx.control.execution.clone();
    let (anchor_tx, anchor_rx) = mpsc::channel::<ConvoEvent>();
    // spawn **전에** 실행을 올린다. 세션 id는 `system/init`에서야 오는데 `ask_user`는 그보다
    // 먼저 올 수 있고, `open`은 실행이 `running`이 아니면 질문을 받지 않는다.
    ctx.db(ledger::started_local(&ctx.pool, &execution, &ledger::id()?))?;
    ctx.changed();
    let registered = {
        sessions().lock().unwrap_or_else(|e| e.into_inner()).insert(
            ctx.task_id,
            Session {
                ctx: ctx.clone(),
                anchors: anchor_tx,
            },
        );
        Registered(ctx.task_id)
    };

    let prompt = format!("{TOOL_INSTRUCTIONS}\n\n# User request\n{message}");
    let spawn_ctx = ctx.clone();
    let result = super::run_turn_with_effort(
        cwd,
        &prompt,
        resume,
        idle_timeout,
        vendor,
        bin,
        model,
        effort,
        service_tier,
        images,
        session_name,
        mcp.map(|lease| lease.injection()),
        move |pid| {
            // 중단이 읽을 자리. 플래그만으로는 이 런타임을 멈출 수 없다.
            spawn_ctx.control.pgid.store(pid, Ordering::SeqCst);
            // 회수 근거. 남기지 못해도 턴은 계속한다 — 없으면 크래시 복구가 약해질 뿐이다.
            if let Ok(Some(identity)) = crate::runner::process_identity::observe_group_leader(pid) {
                let _ = spawn_ctx.db(ledger::spawned_local(
                    &spawn_ctx.pool,
                    &spawn_ctx.control.execution,
                    pid,
                    &identity,
                ));
            }
            on_spawn(pid);
        },
        |event| {
            drain_anchors(&anchor_rx, &mut on_event);
            if let ConvoEvent::SessionInit { session_id } = &event {
                let _ = ctx.db(ledger::thread_bound(&ctx.pool, &execution, session_id));
            }
            on_event(event);
        },
    );

    // 등록을 먼저 거둔다 — 이 아래로는 새 질문이 열리지 않는다.
    drop(registered);
    // 스트림이 끝난 뒤 열린 질문(중단·미응답)도 앵커는 남긴다 — 위치는 어차피 맨 끝이다.
    drain_anchors(&anchor_rx, &mut on_event);
    let interrupted = ctx.control.cancelled.load(Ordering::SeqCst);
    // 미응답 판정은 닫기보다 **먼저다**. 닫고 나면 셀 대상이 사라진다.
    let (unanswered, _) = ctx
        .db(ledger::pending(&ctx.pool, &execution, crate::now()))
        .unwrap_or((0, false));
    // 닫기가 배수보다 **먼저다**. 뒤집으면 대기 중인 MCP 호출이 스스로 풀리지 못해 배수가
    // 5초를 다 쓰고 실패하고, 멀쩡한 턴이 `cleanup_failed`로 잠긴다.
    let closed = ctx.db(ledger::close_questions(
        &ctx.pool,
        &execution,
        if interrupted {
            "cancelled"
        } else {
            "turn_ended"
        },
    ));
    let drained = mcp.is_none_or(|lease| lease.revoke_and_drain(CLOSE_TIMEOUT));
    if closed.is_err() || !drained {
        ctx.control.cleanup_failed.store(true, Ordering::SeqCst);
        let _ = ctx.db(ledger::phase(&ctx.pool, &execution, "cleanup_failed"));
        ctx.changed();
        return Err("질문 정리를 확인하지 못했습니다. 승인·폐기가 잠겨 있습니다".into());
    }

    let outcome = result.and_then(|turn| {
        if interrupted || unanswered == 0 {
            Ok(turn)
        } else {
            Err("응답되지 않은 질문이 남은 채 실행이 종료되었습니다".into())
        }
    });
    let failure = outcome.as_ref().err().cloned();
    if let Err(error) = ctx.db(ledger::finish(
        &ctx.pool,
        &execution,
        if interrupted {
            "cancelled"
        } else if failure.is_none() {
            "completed"
        } else {
            "failed"
        },
        failure.as_deref(),
    )) {
        ctx.control.cleanup_failed.store(true, Ordering::SeqCst);
        return Err(error);
    }
    ctx.changed();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convo::app_server::Control;
    use serde_json::json;
    use std::sync::Arc;

    async fn pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE tasks(id INTEGER PRIMARY KEY,convo_session_id TEXT,pending_capsule TEXT);INSERT INTO tasks(id) VALUES(7)")
            .execute(&pool)
            .await
            .unwrap();
        ledger::migrate(&pool).await.unwrap();
        ledger::bind(&pool, 7).await.unwrap();
        pool
    }

    fn args() -> Value {
        json!({"kind":"clarification","questions":[{"id":"color","question":"색상?","options":[{"id":"blue","label":"파랑","description":""}],"allow_free_text":true,"is_secret":false}]})
    }

    /// 질문 카드가 대화 맨 아래 폴백이 아니라 제자리에 그려지려면 `ask`가 트랜스크립트 앵커를
    /// 남겨야 한다 — Codex 경로의 `open`과 같은 계약. 답이 실린 벤더 이벤트보다 앞에 선다.
    #[tokio::test]
    async fn ask_anchors_the_question_before_the_vendor_event_that_carries_the_answer() {
        const TASK: i64 = 7;
        let pool = pool().await;
        let execution = ledger::begin(&pool, TASK, 100).await.unwrap();
        ledger::started_local(&pool, &execution, "turn")
            .await
            .unwrap();
        let (anchor_tx, anchor_rx) = mpsc::channel::<ConvoEvent>();
        let ctx = Context {
            pool: pool.clone(),
            task_id: TASK,
            control: Arc::new(Control::new(execution.clone(), true)),
            changed: Arc::new(|| {}),
        };
        sessions().lock().unwrap_or_else(|e| e.into_inner()).insert(
            TASK,
            Session {
                ctx,
                anchors: anchor_tx,
            },
        );
        let _registered = Registered(TASK);

        let asking = tokio::spawn(async move { ask(TASK, &args()).await });
        let anchor = tokio::task::spawn_blocking(move || {
            anchor_rx.recv_timeout(Duration::from_secs(5)).unwrap()
        })
        .await
        .unwrap();
        let ConvoEvent::Interaction { interaction_id } = &anchor else {
            panic!("앵커가 질문 이벤트가 아니다: {anchor:?}");
        };
        let snapshot = ledger::snapshot(&pool, TASK).await.unwrap();
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(&snapshot.items[0].id, interaction_id);

        let answer = vec![ledger::Answer {
            question_id: "color".into(),
            option_id: Some("blue".into()),
            text: None,
        }];
        ledger::submit(
            &pool,
            TASK,
            &execution,
            interaction_id,
            "request",
            &answer,
            101,
        )
        .await
        .unwrap();
        let output = asking.await.unwrap().unwrap();
        assert!(output.contains("blue"), "{output}");

        // 앵커는 콜백이 벤더 이벤트를 넘기기 **전에** 배수된다.
        let (tx, rx) = mpsc::channel::<ConvoEvent>();
        tx.send(ConvoEvent::Interaction {
            interaction_id: "q".into(),
        })
        .unwrap();
        let mut seen = Vec::new();
        let mut sink = |event: ConvoEvent| seen.push(event);
        drain_anchors(&rx, &mut sink);
        sink(ConvoEvent::Text {
            text: "answer".into(),
            parent_id: None,
        });
        assert!(matches!(seen[0], ConvoEvent::Interaction { .. }));
        assert!(matches!(seen[1], ConvoEvent::Text { .. }));
    }
}
