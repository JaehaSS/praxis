use sqlx::SqlitePool;
use tokio::sync::broadcast;

use crate::db::{self, RunnerEvent};

pub const REPLAY_LIMIT: i64 = 1_000;

/// DB ledger를 WebSocket 구독자에게 fan-out하는 Runner-local event hub.
#[derive(Clone)]
pub struct EventHub {
    pool: SqlitePool,
    sender: broadcast::Sender<RunnerEvent>,
}

pub struct ReplaySubscription {
    pub watermark: i64,
    pub replay: Vec<RunnerEvent>,
    pub receiver: broadcast::Receiver<RunnerEvent>,
}

impl EventHub {
    pub fn start(pool: SqlitePool) -> Self {
        let (sender, _) = broadcast::channel(1_024);
        let hub = Self { pool, sender };
        hub.spawn_fanout();
        hub
    }

    pub async fn replay_after(&self, after: i64) -> anyhow::Result<Vec<RunnerEvent>> {
        db::list_runner_events_after(&self.pool, after.max(0), REPLAY_LIMIT).await
    }

    /// `after` 초과 ~ `through` 이하 구간을 한 페이지 읽는다.
    /// 첫 replay가 REPLAY_LIMIT에서 잘렸을 때 watermark까지 이어 보내는 데 쓴다.
    pub async fn replay_through(
        &self,
        after: i64,
        through: i64,
    ) -> anyhow::Result<Vec<RunnerEvent>> {
        db::list_runner_events_through(&self.pool, after.max(0), through, REPLAY_LIMIT).await
    }

    /// receiver를 snapshot 전에 만들고 watermark 이후만 live로 전달한다.
    pub async fn subscribe_after(&self, after: i64) -> anyhow::Result<ReplaySubscription> {
        let receiver = self.sender.subscribe();
        let watermark = db::latest_runner_event_sequence(&self.pool).await?;
        let replay =
            db::list_runner_events_through(&self.pool, after.max(0), watermark, REPLAY_LIMIT)
                .await?;
        Ok(ReplaySubscription {
            watermark,
            replay,
            receiver,
        })
    }

    fn spawn_fanout(&self) {
        let pool = self.pool.clone();
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let mut after = 0;
            loop {
                match db::list_runner_events_after(&pool, after, REPLAY_LIMIT).await {
                    Ok(events) => {
                        for event in events {
                            after = event.sequence;
                            let _ = sender.send(event);
                        }
                    }
                    Err(error) => eprintln!("Runner event fan-out 조회 실패: {error}"),
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        });
    }
}
