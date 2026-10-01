use crate::error::HttpError;
use axum::{extract::{rejection::JsonRejection, State}, Extension, Json};
use gfa_api_types::{ApiError, TrainingBatch, TrainingBatchResult};
use gfa_service::GameService;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(crate) struct TrainingWorkers(pub Arc<Semaphore>);

#[utoipa::path(
    post, path = "/v1/batch/step", tag = "Training",
    request_body = TrainingBatch,
    responses(
        (status = 200, description = "Ordered independent owner-only training results; no persistence or match IDs", body = TrainingBatchResult),
        (status = "default", description = "Batch size, local access, or worker limit failure", body = gfa_api_types::ErrorResponse)
    )
)]
pub(crate) async fn batch(
    State(service): State<Arc<GameService>>,
    Extension(workers): Extension<TrainingWorkers>,
    body: Result<Json<TrainingBatch>, JsonRejection>,
) -> Result<Json<TrainingBatchResult>, HttpError> {
    let Json(request) = body?;
    let permit = workers.0.try_acquire_owned().map_err(|_| ApiError::new(
        "ENGINE_BUSY", "All training workers are busy", "Retry this stateless batch later.",
    ))?;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        service.training_batch(request)
    })
    .await
    .map_err(|_| ApiError::new("ENGINE_UNAVAILABLE", "Training worker failed", "Retry this stateless batch."))??;
    Ok(Json(result))
}
