use extraction_server::contracts::{
    bundled_envelope_fixture, bundled_manifest, decode_protocol_envelope, ExtractionManifest,
    ResourceKey,
};
use serde_json::json;

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
}

fn decode_manifest(value: serde_json::Value) -> Result<ExtractionManifest, String> {
    let manifest: ExtractionManifest =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    manifest.validate().map_err(|error| error.to_string())?;
    Ok(manifest)
}
