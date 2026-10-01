use crate::{internal,play_types::StateOutput};
use gfa_api_types::{ApiError,Briefing,MatchState};
use gfa_core::Viewer;

/// Keep live match briefings within prompt budgets using the same state projection
/// as get_state. Synthetic rules/examples already omit tensors and action masks.
pub(crate) fn compact(mut info: Briefing, viewer: Viewer) -> Result<Briefing,ApiError> {
    if let Some(section)=info.sections.iter_mut().find(|section|section.id=="match") {
        let fields=section.data.as_object_mut().ok_or_else(internal)?;
        let state:MatchState=serde_json::from_value(fields.remove("state").ok_or_else(internal)?).map_err(|_|internal())?;
        fields.insert("state".into(),serde_json::to_value(StateOutput::from_state(state,viewer)?).map_err(|_|internal())?);
    }
    info.estimate_tokens().map_err(|_|internal())?;
    Ok(info)
}
