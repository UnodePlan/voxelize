use voxelize::MessageQueues;

use super::{components::ExtractionComp, messaging::queue_method, runtime::GameplayRuntimeContext};
use crate::{
    contracts::{ExtractionStateData, ExtractionStateEnvelope, ExtractionZoneState},
    generation::MapPoint,
    matchmaking::GameplayTimeline,
};

const EXTRACTION_STATE_METHOD: &str = "pvp:v1:extraction-state";

pub(super) struct ExtractionStateAccess<'a> {
    pub context: &'a GameplayRuntimeContext,
    pub timeline: GameplayTimeline,
    pub now: std::time::Duration,
    pub zone_point: MapPoint,
    pub inside: bool,
    pub alive: bool,
    pub eliminated: bool,
    pub extraction: &'a ExtractionComp,
}

pub(super) fn queue_extraction_state(
    queues: &mut MessageQueues,
    client_id: &str,
    state: &ExtractionStateEnvelope,
) {
    queue_method(queues, client_id, EXTRACTION_STATE_METHOD, state);
}

pub(super) fn extraction_state(
    access: ExtractionStateAccess<'_>,
) -> Option<ExtractionStateEnvelope> {
    let zone = ExtractionZoneState {
        center: [
            access.zone_point.x,
            access.zone_point.y,
            access.zone_point.z,
        ],
        radius_blocks: exact_positive_blocks(access.context.config.extraction_radius)?,
        half_height_blocks: exact_positive_blocks(access.context.config.extraction_half_height)?,
    };
    let progress_revision = access.extraction.progress().revision();
    let (phase_revision, data) = if let Some(record) = access.extraction.record() {
        let qualified_at_unix_seconds =
            u32::try_from(record.qualification.qualified_at.unix_timestamp()).ok()?;
        (
            2,
            ExtractionStateData::Pending {
                zone,
                qualified_at_unix_seconds,
            },
        )
    } else if !access.alive || access.eliminated || access.now > access.timeline.hard_deadline {
        (3, ExtractionStateData::Closed {})
    } else if !access.timeline.extraction_open {
        (0, ExtractionStateData::Hidden {})
    } else {
        let required_ms =
            u32::try_from(access.context.config.extraction_hold_duration.as_millis()).ok()?;
        let elapsed_ms = u32::try_from(
            access
                .extraction
                .progress()
                .elapsed(access.now, access.timeline.hard_deadline)
                .min(access.context.config.extraction_hold_duration)
                .as_millis(),
        )
        .ok()?;
        let hard_deadline_unix_seconds =
            u32::try_from(access.timeline.hard_deadline_utc.unix_timestamp()).ok()?;
        (
            1,
            ExtractionStateData::Open {
                zone,
                inside: access.inside,
                elapsed_ms,
                required_ms,
                hard_deadline_unix_seconds,
            },
        )
    };
    let revision = progress_revision
        .checked_mul(4)?
        .checked_add(phase_revision)?;
    ExtractionStateEnvelope::new(
        &access.context.manifest,
        access.context.match_id,
        revision,
        data,
    )
    .ok()
}

fn exact_positive_blocks(value: f32) -> Option<u32> {
    (value.is_finite() && value > 0.0 && value.fract() == 0.0 && value <= u32::MAX as f32)
        .then_some(value as u32)
}
