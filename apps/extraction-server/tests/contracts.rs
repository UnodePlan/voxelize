use extraction_server::contracts::{
    bundled_envelope_fixture, bundled_gameplay_intent_fixture, bundled_manifest,
    bundled_mining_state_fixture, decode_drop_slot_intent, decode_mining_intent,
    decode_mining_state, decode_protocol_envelope, EquipmentKey, ErrorCode, ExtractionManifest,
    ProtocolEnvelope, ResourceKey,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DropSlotPayload {
    slot: usize,
    expected_inventory_revision: u32,
}

#[test]
fn bundled_manifest_has_stable_resource_contracts() {
    let manifest = bundled_manifest().unwrap();
    let keys: Vec<_> = manifest.resources.iter().map(|item| item.key).collect();
    let weights: Vec<_> = manifest
        .resources
        .iter()
        .map(|item| item.score_weight)
        .collect();

    assert_eq!(
        keys,
        vec![ResourceKey::Dirt, ResourceKey::Gold, ResourceKey::Diamond]
    );
    assert_eq!(weights, vec![1, 10, 100]);
    assert_eq!(
        manifest
            .resources
            .iter()
            .map(|item| (item.key, item.voxel_id, item.item_id))
            .collect::<Vec<_>>(),
        vec![
            (ResourceKey::Dirt, 1001, 2001),
            (ResourceKey::Gold, 1002, 2002),
            (ResourceKey::Diamond, 1003, 2003),
        ]
    );
    assert_eq!(
        manifest
            .equipment
            .iter()
            .map(|item| (item.key, item.item_id))
            .collect::<Vec<_>>(),
        vec![
            (EquipmentKey::BasicPickaxe, 2101),
            (EquipmentKey::BasicMeleeWeapon, 2102),
        ]
    );
}

#[test]
fn rust_decoder_matches_every_shared_envelope_case() {
    let manifest = bundled_manifest().unwrap();
    let fixture = bundled_envelope_fixture().unwrap();

    for fixture_case in fixture.cases {
        let decoded = decode_protocol_envelope(fixture_case.value, &manifest);

        assert_eq!(
            decoded.is_ok(),
            fixture_case.accept,
            "fixture case {} did not match its expectation: {:?}",
            fixture_case.name,
            decoded.err()
        );
    }
}

#[test]
fn rust_decoder_rejects_incomplete_manifest_versions_and_error_taxonomies() {
    let manifest = bundled_manifest().unwrap();

    let mut zero_catalog = serde_json::to_value(&manifest).unwrap();
    zero_catalog["catalogVersion"] = json!(0);
    assert!(decode_manifest(zero_catalog).is_err());

    let mut empty_errors = serde_json::to_value(&manifest).unwrap();
    empty_errors["errorCodes"] = json!([]);
    assert!(decode_manifest(empty_errors).is_err());

    let mut unknown_error = serde_json::to_value(&manifest).unwrap();
    unknown_error["errorCodes"] = json!(["CLIENT_INVENTED_ERROR"]);
    assert!(decode_manifest(unknown_error).is_err());

    let mut oversized_item = serde_json::to_value(&manifest).unwrap();
    oversized_item["resources"][0]["itemId"] = json!(2_147_483_648_u64);
    assert!(decode_manifest(oversized_item).is_err());
}

#[test]
fn typed_intent_owns_top_level_sequence_and_rejects_unknown_payload_fields() {
    let manifest = bundled_manifest().unwrap();
    let request_id = Uuid::from_u128(1);
    let envelope = decode_protocol_envelope(
        json!({
            "protocolVersion": 1,
            "type": "intent",
            "requestId": request_id,
            "sequence": 42,
            "payload": {
                "slot": 3,
                "expectedInventoryRevision": 7
            }
        }),
        &manifest,
    )
    .unwrap();
    let intent = envelope.decode_intent::<DropSlotPayload>().unwrap();
    assert_eq!(intent.request_id, request_id);
    assert_eq!(intent.sequence, 42);
    assert_eq!(
        intent.payload,
        DropSlotPayload {
            slot: 3,
            expected_inventory_revision: 7,
        }
    );

    let envelope = decode_protocol_envelope(
        json!({
            "protocolVersion": 1,
            "type": "intent",
            "requestId": request_id,
            "sequence": 43,
            "payload": {
                "slot": 3,
                "expectedInventoryRevision": 7,
                "quantity": 999
            }
        }),
        &manifest,
    )
    .unwrap();
    assert!(envelope.decode_intent::<DropSlotPayload>().is_err());
}

#[test]
fn result_helpers_emit_valid_shared_envelopes() {
    let manifest = bundled_manifest().unwrap();
    let request_id = Uuid::from_u128(2);
    let ok = ProtocolEnvelope::ok(&manifest, request_id, json!({ "revision": 9 })).unwrap();
    let error = ProtocolEnvelope::error(&manifest, request_id, ErrorCode::GameStaleRevision, false)
        .unwrap();

    for result in [ok, error] {
        let value = serde_json::to_value(result).unwrap();
        assert!(decode_protocol_envelope(value, &manifest).is_ok());
    }
}

#[test]
fn rust_drop_slot_decoder_matches_shared_gameplay_cases() {
    let manifest = bundled_manifest().unwrap();
    let fixture = bundled_gameplay_intent_fixture().unwrap();
    for fixture_case in fixture.cases {
        let decoded =
            decode_protocol_envelope(fixture_case.value, &manifest).is_ok_and(|envelope| {
                match fixture_case.route.as_str() {
                    "pvp:v1:drop-slot" => decode_drop_slot_intent(&envelope).is_ok(),
                    "pvp:v1:mining" => decode_mining_intent(&envelope).is_ok(),
                    _ => false,
                }
            });
        assert_eq!(
            decoded, fixture_case.accept,
            "gameplay fixture case {} did not match",
            fixture_case.name
        );
    }
}

#[test]
fn rust_decoder_matches_shared_mining_state_cases() {
    let manifest = bundled_manifest().unwrap();
    let fixture = bundled_mining_state_fixture().unwrap();
    for fixture_case in fixture.cases {
        let decoded = decode_mining_state(fixture_case.value, &manifest);
        assert_eq!(
            decoded.is_ok(),
            fixture_case.accept,
            "mining state fixture case {} did not match: {:?}",
            fixture_case.name,
            decoded.err()
        );
    }
}

fn decode_manifest(value: serde_json::Value) -> Result<ExtractionManifest, String> {
    let manifest: ExtractionManifest =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    manifest.validate().map_err(|error| error.to_string())?;
    Ok(manifest)
}
