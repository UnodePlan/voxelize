use std::time::Duration;

use super::extraction::{
    freeze_inventory_for_extraction, ExtractionProgress, ExtractionProgressError,
    ExtractionProgressOutcome, ExtractionZone,
};
use crate::{contracts::ResourceKey, gameplay::inventory::MatchInventory};
use time::OffsetDateTime;
use uuid::Uuid;

#[test]
fn extraction_requires_one_continuous_eight_second_window() {
    let mut progress = ExtractionProgress::default();
    let deadline = Duration::from_secs(720);
    let required = Duration::from_secs(8);

    assert_eq!(
        progress.observe(Duration::from_secs(480), deadline, true, true, required),
        Ok(ExtractionProgressOutcome::Started {
            entered_at: Duration::from_secs(480)
        })
    );
    assert!(matches!(
        progress.observe(
            Duration::from_millis(487_999),
            deadline,
            true,
            true,
            required
        ),
        Ok(ExtractionProgressOutcome::Accumulating { .. })
    ));
    assert_eq!(
        progress.observe(Duration::from_secs(488), deadline, true, true, required),
        Ok(ExtractionProgressOutcome::Qualified {
            qualified_at: Duration::from_secs(488)
        })
    );
}

#[test]
fn leaving_death_or_disconnect_each_reset_the_full_hold_window() {
    let deadline = Duration::from_secs(720);
    let required = Duration::from_secs(8);

    for (label, eligible, inside) in [
        ("离开区域", true, false),
        ("死亡", false, true),
        ("断线", false, true),
    ] {
        let mut progress = ExtractionProgress::default();
        progress
            .observe(Duration::from_secs(500), deadline, true, true, required)
            .unwrap();
        assert_eq!(
            progress.observe(
                Duration::from_secs(504),
                deadline,
                eligible,
                inside,
                required
            ),
            Ok(ExtractionProgressOutcome::Reset),
            "{label}必须清零既有进度"
        );
        assert_eq!(
            progress.observe(Duration::from_secs(505), deadline, true, true, required),
            Ok(ExtractionProgressOutcome::Started {
                entered_at: Duration::from_secs(505)
            }),
            "{label}后重新进入必须开启新的连续窗口"
        );
        assert!(matches!(
            progress.observe(
                Duration::from_millis(512_999),
                deadline,
                true,
                true,
                required
            ),
            Ok(ExtractionProgressOutcome::Accumulating { .. })
        ));
        assert_eq!(
            progress.observe(Duration::from_secs(513), deadline, true, true, required),
            Ok(ExtractionProgressOutcome::Qualified {
                qualified_at: Duration::from_secs(513)
            })
        );
    }
}

#[test]
fn hard_deadline_is_inclusive_but_cannot_extend_progress() {
    let deadline = Duration::from_secs(720);
    let required = Duration::from_secs(8);
    let mut qualified = ExtractionProgress::default();
    qualified
        .observe(Duration::from_secs(712), deadline, true, true, required)
        .unwrap();
    assert_eq!(
        qualified.observe(Duration::from_secs(721), deadline, true, true, required),
        Ok(ExtractionProgressOutcome::Qualified {
            qualified_at: deadline
        })
    );

    let mut late = ExtractionProgress::default();
    late.observe(
        Duration::from_millis(712_001),
        deadline,
        true,
        true,
        required,
    )
    .unwrap();
    assert_eq!(
        late.observe(Duration::from_secs(721), deadline, true, true, required),
        Ok(ExtractionProgressOutcome::Expired)
    );
}

#[test]
fn extraction_zone_is_a_bounded_cylinder() {
    let zone = ExtractionZone::new([4.0, 50.0, -2.0], 4.0, 3.0).unwrap();
    assert!(zone.contains([8.0, 53.0, -2.0]));
    assert!(!zone.contains([8.001, 50.0, -2.0]));
    assert!(!zone.contains([4.0, 53.001, -2.0]));
    assert!(!zone.contains([f32::NAN, 50.0, -2.0]));
    assert_eq!(
        ExtractionZone::new([0.0; 3], 0.0, 1.0),
        Err(ExtractionProgressError::InvalidConfig)
    );
}

#[test]
fn qualification_freezes_inventory_and_aggregates_slot_layout() {
    let mut inventory = MatchInventory::new(64).unwrap();
    inventory.insert(ResourceKey::Gold, 65).unwrap();
    inventory.insert(ResourceKey::Dirt, 3).unwrap();
    let qualification = freeze_inventory_for_extraction(
        &mut inventory,
        Uuid::new_v4(),
        Uuid::new_v4(),
        OffsetDateTime::UNIX_EPOCH,
        "balance-v1",
    )
    .unwrap();

    assert!(inventory.is_frozen());
    assert_eq!(qualification.resources.dirt, 3);
    assert_eq!(qualification.resources.gold, 65);
    assert_eq!(qualification.resources.diamond, 0);
    assert_eq!(
        qualification.inventory_digest,
        qualification.resources.digest()
    );
}
