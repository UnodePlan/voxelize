use crate::contracts::bundled_manifest;

use super::{EngineCatalog, StackableItem};

#[test]
fn catalog_uses_stable_ids_and_stack_components() {
    let catalog = EngineCatalog::from_manifest(&bundled_manifest().unwrap()).unwrap();
    let resources = catalog.resources();

    assert_eq!(catalog.blocks().blocks_by_id.len(), 4);
    for (name, voxel_id, item_id) in [
        ("dirt", 1001, 2001),
        ("gold", 1002, 2002),
        ("diamond", 1003, 2003),
    ] {
        assert_eq!(catalog.blocks().get_id_by_name(name), voxel_id);
        assert_eq!(catalog.blocks().get_block_by_id(voxel_id).name, name);
        let item = catalog.items().get_by_id(item_id).unwrap();
        assert_eq!(item.name, name);
        assert_eq!(
            item.get::<StackableItem>(),
            Some(&StackableItem { max_stack: 64 })
        );
        assert_eq!(
            item.to_client_json()["components"]["stackable"]["maxStack"],
            64
        );
    }
    for (name, item_id) in [("basic_pickaxe", 2101), ("basic_melee_weapon", 2102)] {
        let item = catalog.items().get_by_id(item_id).unwrap();
        assert_eq!(item.name, name);
        assert!(!item.has::<StackableItem>());
        assert!(item.to_client_json()["components"]["stackable"].is_null());
    }
    assert_eq!(catalog.items().count(), 5);
    assert_eq!(resources.dirt.score_weight, 1);
    assert_eq!(resources.gold.score_weight, 10);
    assert_eq!(resources.diamond.score_weight, 100);
}

#[test]
fn manifest_array_order_does_not_change_catalog_identity() {
    let mut manifest = bundled_manifest().unwrap();
    manifest.resources.reverse();
    manifest.equipment.reverse();

    let catalog = EngineCatalog::from_manifest(&manifest).unwrap();
    assert_eq!(catalog.blocks().get_id_by_name("dirt"), 1001);
    assert_eq!(catalog.blocks().get_id_by_name("gold"), 1002);
    assert_eq!(catalog.blocks().get_id_by_name("diamond"), 1003);
    assert_eq!(catalog.items().get_id_by_name("basic_pickaxe"), Some(2101));
    assert_eq!(
        catalog.items().get_id_by_name("basic_melee_weapon"),
        Some(2102)
    );
}

#[test]
fn unsupported_catalog_or_generation_version_fails_closed() {
    let mut manifest = bundled_manifest().unwrap();
    manifest.catalog_version = 2;
    assert!(EngineCatalog::from_manifest(&manifest).is_err());

    let mut manifest = bundled_manifest().unwrap();
    manifest.generation_version = "generation-v2".to_owned();
    assert!(EngineCatalog::from_manifest(&manifest).is_err());
}
