use std::time::Duration;

use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    components::ExtractionComp,
    extraction_messaging::{extraction_state, ExtractionStateAccess},
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::{bundled_manifest, ExtractionStateData},
    gameplay::{config::GAMEPLAY_V1, extraction::ExtractionProgressOutcome},
    generation::MapPoint,
    match_world::PlayableBounds,
    matchmaking::{ExtractionQualification, GameplayTimeline, SettlementResources},
};

fn context() -> GameplayRuntimeContext {
    GameplayRuntimeContext::new(
        Uuid::from_u128(1),
        GAMEPLAY_V1,
        bundled_manifest().unwrap(),
        PlayableBounds::EXTRACTION,
        1,
        64,
        16,
    )
}

fn timeline(open: bool) -> GameplayTimeline {
    GameplayTimeline {
        extraction_open: open,
        hard_deadline: Duration::from_secs(720),
        hard_deadline_utc: OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
    }
}

#[test]
fn extraction_state_hides_zone_then_publishes_progress_and_pending() {
    let context = context();
    let zone = MapPoint::new(17, 42, -8);
    let mut extraction = ExtractionComp::default();
    let hidden = extraction_state(ExtractionStateAccess {
        context: &context,
        timeline: timeline(false),
        now: Duration::from_secs(479),
        zone_point: zone,
        inside: false,
        alive: true,
        eliminated: false,
        extraction: &extraction,
    })
    .unwrap();
    assert!(matches!(hidden.data, ExtractionStateData::Hidden {}));
    assert!(serde_json::to_value(hidden).unwrap()["data"]
        .get("zone")
        .is_none());

    assert!(matches!(
        extraction.progress_mut().observe(
            Duration::from_secs(480),
            Duration::from_secs(720),
            true,
            true,
            Duration::from_secs(8),
        ),
        Ok(ExtractionProgressOutcome::Started { .. })
    ));
    extraction
        .progress_mut()
        .observe(
            Duration::from_secs(484),
            Duration::from_secs(720),
            true,
            true,
            Duration::from_secs(8),
        )
        .unwrap();
    let open = extraction_state(ExtractionStateAccess {
        context: &context,
        timeline: timeline(true),
        now: Duration::from_secs(484),
        zone_point: zone,
        inside: true,
        alive: true,
        eliminated: false,
        extraction: &extraction,
    })
    .unwrap();
    assert!(matches!(
        open.data,
        ExtractionStateData::Open {
            zone,
            inside: true,
            elapsed_ms: 4_000,
            required_ms: 8_000,
            ..
        } if zone.center == [17, 42, -8]
    ));
    assert!(extraction.should_publish(open.revision));
    assert!(!extraction.should_publish(open.revision));

    let qualified_at = context.config.extraction_hold_duration + Duration::from_secs(480);
    assert!(matches!(
        extraction.progress_mut().observe(
            qualified_at,
            Duration::from_secs(720),
            true,
            true,
            Duration::from_secs(8),
        ),
        Ok(ExtractionProgressOutcome::Qualified { .. })
    ));
    let qualification = ExtractionQualification::new(
        context.match_id,
        Uuid::from_u128(2),
        OffsetDateTime::from_unix_timestamp(1_799_999_768).unwrap(),
        SettlementResources::new(3, 2, 1),
        context.config.config_version.to_owned(),
    )
    .unwrap();
    assert!(extraction.qualify(qualification));
    let pending = extraction_state(ExtractionStateAccess {
        context: &context,
        timeline: timeline(true),
        now: qualified_at,
        zone_point: zone,
        inside: true,
        alive: true,
        eliminated: false,
        extraction: &extraction,
    })
    .unwrap();
    assert!(matches!(pending.data, ExtractionStateData::Pending { .. }));
    assert!(pending.revision > open.revision);
    assert!(extraction.should_publish(pending.revision));
}
