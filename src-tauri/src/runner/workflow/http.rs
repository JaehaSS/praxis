//! This router is merged before the existing Runner authentication middleware.
use super::WorkflowService;
use crate::{runner::http::RunnerHttpState, workflow::WorkflowSpec};
use axum::{
    extract::{DefaultBodyLimit, Path, Query},
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct WorkflowEndpoint(pub Option<Arc<WorkflowService>>);
type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

pub fn routes(service: Option<Arc<WorkflowService>>) -> Router<RunnerHttpState> {
    Router::new()
        .route("/v1/workflows", get(list).post(create))
        .route("/v1/workflows/:id", get(snapshot))
        .route("/v1/workflows/:id/events", get(events))
        .route("/v1/workflows/:id/steps", get(steps))
        .route("/v1/workflows/:id/logs/:digest", get(log))
        .route("/v1/workflows/:id/validate", post(validate))
        .route("/v1/workflows/:id/start", post(start))
        .route("/v1/workflows/:id/pause", post(pause))
        .route("/v1/workflows/:id/resume", post(resume))
        .route("/v1/workflows/:id/cancel", post(cancel))
        .route("/v1/workflows/:id/revisions", post(revise))
        .route("/v1/workflows/:id/nodes/:node/retry", post(retry))
        .route("/v1/workflows/:id/nodes/:node/accept", post(accept))
        .route("/v1/workflows/:id/artifacts", get(artifacts))
        .route("/v1/workflows/:id/artifacts/:artifact", get(artifact))
        .route("/v1/workflows/:id/repair", post(repair))
        .layer(DefaultBodyLimit::max(300 * 1024))
        .layer(Extension(WorkflowEndpoint(service)))
}
fn service(endpoint: WorkflowEndpoint) -> Result<Arc<WorkflowService>, (StatusCode, Json<Value>)> {
    endpoint.0.ok_or_else(||(StatusCode::UNPROCESSABLE_ENTITY,Json(json!({"code":"capability_unavailable","error":"Workflow is disabled; configure PRAXIS_WORKFLOW_CONFIG on the Linux Runner"}))))
}
fn error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    let message = error.to_string();
    let (status, code) = if message.contains("capability_unavailable") {
        (StatusCode::UNPROCESSABLE_ENTITY, "capability_unavailable")
    } else if message.contains("not found") {
        (StatusCode::NOT_FOUND, "not_found")
    } else {
        (StatusCode::CONFLICT, "workflow_conflict")
    };
    (status, Json(json!({"code":code,"error":message})))
}
fn receipt(id: &str, request_id: &str, revision: i64) -> Json<Value> {
    Json(json!({"workflow_id":id,"request_id":request_id,"revision":revision,"accepted":true}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    request_id: String,
    spec: WorkflowSpec,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    request_id: String,
    expected_revision: i64,
    #[serde(default)]
    approved_scope_hash: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    request_id: String,
    expected_revision: i64,
    spec: WorkflowSpec,
}
#[derive(Deserialize)]
struct After {
    #[serde(default)]
    after: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Accept {
    request_id: String,
    expected_revision: i64,
    attempt_id: i64,
    snapshot_hash: String,
    criterion: String,
}

async fn list(Extension(endpoint): Extension<WorkflowEndpoint>) -> ApiResult {
    Ok(Json(json!(service(endpoint)?
        .list()
        .await
        .map_err(error)?)))
}
async fn snapshot(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
) -> ApiResult {
    Ok(Json(
        service(endpoint)?
            .store
            .snapshot(&id)
            .await
            .map_err(error)?,
    ))
}
async fn events(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
    Query(query): Query<After>,
) -> ApiResult {
    let service = service(endpoint)?;
    service.store.run(&id).await.map_err(error)?;
    Ok(Json(json!(service
        .store
        .events_after(&id, query.after)
        .await
        .map_err(error)?)))
}
async fn steps(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
) -> ApiResult {
    let service = service(endpoint)?;
    service.store.run(&id).await.map_err(error)?;
    Ok(Json(json!(service
        .store
        .steps(&id)
        .await
        .map_err(error)?)))
}
async fn log(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path((id, digest)): Path<(String, String)>,
) -> Result<(axum::http::HeaderMap, Vec<u8>), (StatusCode, Json<Value>)> {
    let service = service(endpoint)?;
    if !service.store.owns_log(&id, &digest).await.map_err(error)? {
        return Err(error(anyhow::anyhow!("workflow log not found")));
    }
    let bytes = super::logs::read(&service.config.workspace_root, &digest).map_err(error)?;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("content-type", "text/plain; charset=utf-8".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("cache-control", "no-store".parse().unwrap());
    Ok((headers, bytes))
}
async fn create(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Json(body): Json<Create>,
) -> ApiResult {
    let (id, revision) = service(endpoint)?
        .create(&body.spec, &body.request_id)
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, revision))
}
async fn validate(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> ApiResult {
    let admission = service(endpoint)?
        .validate(&id, body.expected_revision, &body.request_id)
        .await
        .map_err(error)?;
    Ok(Json(
        json!({"workflow_id":id,"request_id":body.request_id,"revision":body.expected_revision,"accepted":true,"approved_scope_hash":admission.scope_hash}),
    ))
}
async fn start(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> ApiResult {
    service(endpoint)?
        .start(
            &id,
            body.expected_revision,
            &body.request_id,
            body.approved_scope_hash.as_deref(),
            false,
        )
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, body.expected_revision))
}
async fn resume(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> ApiResult {
    service(endpoint)?
        .start(
            &id,
            body.expected_revision,
            &body.request_id,
            body.approved_scope_hash.as_deref(),
            true,
        )
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, body.expected_revision))
}
async fn pause(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> ApiResult {
    service(endpoint)?
        .store
        .pause(
            &id,
            body.expected_revision,
            &body.request_id,
            crate::runner::now_secs(),
        )
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, body.expected_revision))
}
async fn cancel(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> ApiResult {
    service(endpoint)?
        .cancel(&id, body.expected_revision, &body.request_id)
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, body.expected_revision))
}
async fn revise(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
    Json(body): Json<Revision>,
) -> ApiResult {
    let service = service(endpoint)?;
    service.config.validate_spec(&body.spec).map_err(error)?;
    let result = service
        .store
        .apply_revision(
            &id,
            body.expected_revision,
            &body.request_id,
            &body.spec,
            crate::runner::now_secs(),
        )
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, result.revision))
}
async fn retry(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path((id, node)): Path<(String, String)>,
    Json(body): Json<Action>,
) -> ApiResult {
    let service = service(endpoint)?;
    service
        .retry(&id, body.expected_revision, &node, &body.request_id)
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, body.expected_revision))
}
async fn accept(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path((id, node)): Path<(String, String)>,
    Json(body): Json<Accept>,
) -> ApiResult {
    service(endpoint)?
        .accept(
            &id,
            body.expected_revision,
            &node,
            body.attempt_id,
            &body.snapshot_hash,
            &body.criterion,
            &body.request_id,
        )
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, body.expected_revision))
}
async fn artifact(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path((id, artifact)): Path<(String, String)>,
) -> ApiResult {
    Ok(Json(
        service(endpoint)?
            .artifact(&id, &artifact)
            .await
            .map_err(error)?,
    ))
}
async fn artifacts(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
) -> ApiResult {
    let service = service(endpoint)?;
    service.store.run(&id).await.map_err(error)?;
    Ok(Json(json!(service
        .store
        .artifacts(&id)
        .await
        .map_err(error)?)))
}
async fn repair(
    Extension(endpoint): Extension<WorkflowEndpoint>,
    Path(id): Path<String>,
    Json(body): Json<Action>,
) -> ApiResult {
    service(endpoint)?
        .repair(&id, body.expected_revision, &body.request_id)
        .await
        .map_err(error)?;
    Ok(receipt(&id, &body.request_id, body.expected_revision))
}
