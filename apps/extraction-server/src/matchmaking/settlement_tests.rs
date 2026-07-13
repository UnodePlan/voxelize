use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{ExtractionQualification, SettlementResources};

#[test]
fn canonical_inventory_digest_has_fixed_keys_and_order() {
    let resources = SettlementResources::new(64, 2, 1);
    let canonical = "{\"diamond\":1,\"dirt\":64,\"gold\":2}";
    assert_eq!(resources.canonical_json(), canonical);
    assert_eq!(
        resources.digest(),
        <[u8; 32]>::from(Sha256::digest(canonical))
    );
    assert_eq!(resources.total_value("balance-v1"), Some(184));
    assert_eq!(resources.total_value("balance-v2"), None);
}

#[test]
fn empty_inventory_is_still_a_valid_idempotent_qualification() {
    let match_id = Uuid::new_v4();
    let account_id = Uuid::new_v4();
    let qualification = ExtractionQualification::new(
        match_id,
        account_id,
        OffsetDateTime::UNIX_EPOCH,
        SettlementResources::default(),
        "balance-v1".to_owned(),
    )
    .unwrap();

    assert!(qualification.resources.is_empty());
    assert_eq!(
        qualification.idempotency_key(),
        format!("extract:v1:{match_id}:{account_id}")
    );
    assert_eq!(qualification.resources.total_value("balance-v1"), Some(0));
}

#[test]
fn settlement_value_overflow_is_rejected() {
    assert_eq!(
        SettlementResources::new(0, 0, u64::MAX).total_value("balance-v1"),
        None
    );
}
