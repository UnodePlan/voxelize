pub(super) const ROLE_PROBE_SQL: &str = "WITH target_tables(table_name) AS (VALUES \
         ('public.accounts'), ('public.matches'), ('public.match_participants'), \
         ('public.extraction_settlements'), ('public.settlement_items'), \
         ('public.warehouse_balances'), ('public.asset_ledger') \
     ), member_roles AS ( \
         SELECT role.oid, role.rolname, role.rolsuper, role.rolcreatedb, role.rolcreaterole, \
                role.rolreplication, role.rolbypassrls \
         FROM pg_roles AS role \
         WHERE role.rolname = session_user \
            OR pg_has_role(session_user, role.oid, 'MEMBER') \
     ) \
     SELECT current_user = session_user AS identities_match, \
            COALESCE(( \
                SELECT bool_and(has_table_privilege( \
                    session_user, target.table_name, 'SELECT' \
                )) FROM target_tables AS target \
            ), false) AS all_targets_readable, \
            NOT EXISTS ( \
                SELECT 1 FROM member_roles AS role \
                CROSS JOIN target_tables AS target \
                WHERE has_table_privilege(role.oid, target.table_name, 'INSERT') \
                   OR has_table_privilege(role.oid, target.table_name, 'UPDATE') \
                   OR has_table_privilege(role.oid, target.table_name, 'DELETE') \
                   OR has_table_privilege(role.oid, target.table_name, 'TRUNCATE') \
                   OR has_table_privilege(role.oid, target.table_name, 'TRIGGER') \
                   OR has_table_privilege(role.oid, target.table_name, 'REFERENCES') \
            ) AS no_table_writes, \
            NOT EXISTS ( \
                SELECT 1 FROM member_roles AS role \
                CROSS JOIN target_tables AS target \
                WHERE has_any_column_privilege(role.oid, target.table_name, 'INSERT') \
                   OR has_any_column_privilege(role.oid, target.table_name, 'UPDATE') \
                   OR has_any_column_privilege(role.oid, target.table_name, 'REFERENCES') \
            ) AS no_column_writes, \
            NOT EXISTS ( \
                SELECT 1 FROM member_roles AS role \
                WHERE left(role.rolname, 3) = 'pg_' \
                   OR role.rolsuper OR role.rolcreatedb OR role.rolcreaterole \
                   OR role.rolreplication OR role.rolbypassrls \
            ) AS no_dangerous_memberships";

#[derive(Clone, Copy, Debug, sqlx::FromRow)]
pub(super) struct RoleProbeRow {
    pub(super) identities_match: bool,
    pub(super) all_targets_readable: bool,
    pub(super) no_table_writes: bool,
    pub(super) no_column_writes: bool,
    pub(super) no_dangerous_memberships: bool,
}

impl RoleProbeRow {
    pub(super) const fn is_safe(self) -> bool {
        self.identities_match
            && self.all_targets_readable
            && self.no_table_writes
            && self.no_column_writes
            && self.no_dangerous_memberships
    }
}
