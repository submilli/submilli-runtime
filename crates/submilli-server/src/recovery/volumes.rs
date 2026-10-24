use super::*;
use crate::application::error::StoreError;
use crate::application::sessions::recovery::RunStatusFilter;
use submilli_blueprint::Access;

pub(super) async fn writable_volumes(
    state: &AppState,
    id: &RunId,
) -> Result<HashSet<String>, StoreError> {
    let mut unit = state.units().begin().await?;
    let program = unit
        .get_run_program(id)
        .await?
        .ok_or_else(|| StoreError::Io("run program is missing".into()))?;
    let volumes = state.volumes();
    // Names are literal blueprint identities; subPath variables only narrow
    // the directory. Reserve the whole volume even for disjoint subdirectories.
    Ok(program
        .blueprint
        .vfs
        .named_references()
        .into_iter()
        .filter(|reference| reference.access != Some(Access::ReadOnly))
        .filter(|reference| {
            volumes
                .get(reference.volume)
                .is_none_or(|volume| volume.access != Access::ReadOnly)
        })
        .map(|reference| reference.volume.to_owned())
        .collect())
}

pub(super) async fn has_prior_recovering_writer(
    state: &AppState,
    candidate: &Run,
    writable: &HashSet<String>,
) -> Result<bool, StoreError> {
    if writable.is_empty() {
        return Ok(false);
    }
    let mut after = None;
    loop {
        let mut unit = state.units().begin().await?;
        let page = unit
            .list_runs(RunQuery {
                status: Some(RunStatusFilter::Running),
                recovery: Some(RecoveryFilter::Execution),
                after,
                limit: 100,
                ..Default::default()
            })
            .await?;
        drop(unit);
        after = next_page(&page);
        for running in page {
            if running.reservation_id() == candidate.reservation_id() {
                continue;
            }
            // Recover persisted running work before queued writers, regardless
            // of the scan cursor. If an older server left several running,
            // drain them in run-ID order instead of deadlocking their recovery.
            if candidate.status() == RunStatus::Running
                && running.reservation_id() > candidate.reservation_id()
            {
                continue;
            }
            let running_volumes = match writable_volumes(state, running.reservation_id()).await {
                Ok(volumes) => volumes,
                Err(error) => {
                    // Its unknown volumes withhold this writer, but must not
                    // reset discovery before the damaged run is visited and
                    // marked stuck by schedule's candidate error handling.
                    tracing::warn!(run = %running.reservation_id(), %error, "waiting for recovering writer's volume configuration");
                    return Ok(true);
                }
            };
            if !writable.is_disjoint(&running_volumes) {
                return Ok(true);
            }
        }
        if after.is_none() {
            return Ok(false);
        }
    }
}
