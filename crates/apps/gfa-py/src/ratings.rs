use crate::error;
use gfa_service::ratings::{rate_pair, RatedResult, Rating};
use pyo3::prelude::*;

fn parse(text: &str) -> PyResult<Rating> {
    if text.len() > 4096 {
        return Err(error("rating payload exceeds 4096 bytes"));
    }
    let rating: Rating = serde_json::from_str(text).map_err(error)?;
    rating.validate().map_err(error)
}

/// Validate a serialized rating without updating its uncertainty.
#[pyfunction]
pub(crate) fn rating_validate_json(rating: &str) -> PyResult<()> {
    parse(rating).map(|_| ())
}

/// Apply the same pure rating period used by the service domain.
#[pyfunction]
#[pyo3(signature = (rating, results, tau=0.5))]
pub(crate) fn rating_period_json(py: Python<'_>, rating: &str, results: &str, tau: f64) -> PyResult<String> {
    if results.len() > 1024 * 1024 {
        return Err(error("rating results exceed 1 MiB"));
    }
    let rating = parse(rating)?;
    let inputs: Vec<(Rating, f64)> = serde_json::from_str(results).map_err(error)?;
    py.detach(|| {
        let results: Vec<_> = inputs.into_iter().map(|(opponent, score)| RatedResult { opponent, score }).collect();
        let updated = rating.update_period(&results, tau).map_err(error)?;
        serde_json::to_string(&updated).map_err(error)
    })
}

/// Rate both seats from the original values, optionally preserving calibrated anchors.
#[pyfunction]
#[pyo3(signature = (left, right, score, fixed_left=false, fixed_right=false, tau=0.5))]
pub(crate) fn rating_pair_json(left: &str, right: &str, score: f64, fixed_left: bool, fixed_right: bool, tau: f64) -> PyResult<String> {
    let pair = rate_pair(parse(left)?, parse(right)?, score, fixed_left, fixed_right, tau).map_err(error)?;
    serde_json::to_string(&pair).map_err(error)
}
