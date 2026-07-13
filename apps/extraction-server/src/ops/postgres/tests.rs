use super::{RoleProbeRow, ROLE_PROBE_SQL};

#[test]
fn role_probe_requires_every_independent_safety_dimension() {
    let safe = RoleProbeRow {
        identities_match: true,
        all_targets_readable: true,
        no_table_writes: true,
        no_column_writes: true,
        no_dangerous_memberships: true,
    };
    assert!(safe.is_safe());

    let unsafe_cases = [
        RoleProbeRow {
            identities_match: false,
            ..safe
        },
        RoleProbeRow {
            all_targets_readable: false,
            ..safe
        },
        RoleProbeRow {
            no_table_writes: false,
            ..safe
        },
        RoleProbeRow {
            no_column_writes: false,
            ..safe
        },
        RoleProbeRow {
            no_dangerous_memberships: false,
            ..safe
        },
    ];
    assert!(unsafe_cases.into_iter().all(|probe| !probe.is_safe()));
}

#[test]
fn role_probe_sql_covers_column_grants_identity_and_role_membership() {
    for table in [
        "public.accounts",
        "public.matches",
        "public.match_participants",
        "public.extraction_settlements",
        "public.settlement_items",
        "public.warehouse_balances",
        "public.asset_ledger",
    ] {
        assert!(ROLE_PROBE_SQL.contains(table));
    }
    for privilege in ["'INSERT'", "'UPDATE'", "'REFERENCES'"] {
        assert!(ROLE_PROBE_SQL.contains(&format!(
            "has_any_column_privilege(role.oid, target.table_name, {privilege})"
        )));
    }
    assert_eq!(
        ROLE_PROBE_SQL.matches("has_any_column_privilege").count(),
        3
    );
    assert!(ROLE_PROBE_SQL.contains("current_user = session_user"));
    assert!(ROLE_PROBE_SQL.contains("pg_has_role(session_user, role.oid, 'MEMBER')"));
    assert!(ROLE_PROBE_SQL.contains("left(role.rolname, 3) = 'pg_'"));
    for dangerous_attribute in [
        "rolsuper",
        "rolcreatedb",
        "rolcreaterole",
        "rolreplication",
        "rolbypassrls",
    ] {
        assert!(ROLE_PROBE_SQL.contains(dangerous_attribute));
    }
    assert!(!ROLE_PROBE_SQL.contains("transaction_read_only"));
}
