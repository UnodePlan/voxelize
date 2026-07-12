CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'disabled')),
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CHECK (updated_at >= created_at)
);

CREATE TABLE wallet_credentials (
    account_id UUID NOT NULL REFERENCES accounts(id),
    chain_id BIGINT NOT NULL CHECK (chain_id = 1),
    address BYTEA NOT NULL CHECK (octet_length(address) = 20),
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (account_id, chain_id, address),
    UNIQUE (chain_id, address)
);

CREATE TABLE auth_nonces (
    id UUID PRIMARY KEY,
    nonce_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(nonce_hash) = 32),
    domain TEXT NOT NULL CHECK (length(domain) > 0),
    uri TEXT NOT NULL CHECK (length(uri) > 0),
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    CHECK (expires_at > created_at),
    CHECK (consumed_at IS NULL OR consumed_at >= created_at)
);

CREATE TABLE auth_sessions (
    id UUID PRIMARY KEY,
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    account_id UUID NOT NULL,
    chain_id BIGINT NOT NULL CHECK (chain_id = 1),
    address BYTEA NOT NULL CHECK (octet_length(address) = 20),
    expires_at TIMESTAMPTZ NOT NULL,
    idle_expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL,
    FOREIGN KEY (account_id, chain_id, address)
        REFERENCES wallet_credentials(account_id, chain_id, address),
    CHECK (expires_at > created_at),
    CHECK (idle_expires_at > created_at AND idle_expires_at <= expires_at),
    CHECK (last_seen_at >= created_at),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE INDEX auth_nonces_expiry_idx
    ON auth_nonces (expires_at) WHERE consumed_at IS NULL;
CREATE INDEX auth_sessions_account_idx ON auth_sessions (account_id);
CREATE INDEX auth_sessions_expiry_idx
    ON auth_sessions (expires_at) WHERE revoked_at IS NULL;
CREATE UNIQUE INDEX auth_sessions_active_account_idx
    ON auth_sessions (account_id) WHERE revoked_at IS NULL;

CREATE TABLE matches (
    id UUID PRIMARY KEY,
    state TEXT NOT NULL CHECK (
        state IN ('waiting', 'preparing', 'active', 'extraction_open', 'settling', 'finished', 'aborted')
    ),
    world_name TEXT NOT NULL UNIQUE CHECK (length(world_name) > 0),
    seed BIGINT NOT NULL CHECK (seed >= 0),
    generation_version TEXT NOT NULL CHECK (length(generation_version) > 0),
    gameplay_version TEXT NOT NULL CHECK (length(gameplay_version) > 0),
    config_version TEXT NOT NULL CHECK (length(config_version) > 0),
    created_at TIMESTAMPTZ NOT NULL,
    started_at TIMESTAMPTZ,
    extraction_open_at TIMESTAMPTZ,
    hard_deadline TIMESTAMPTZ,
    settlement_grace_deadline TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    abort_reason TEXT,
    CHECK (started_at IS NULL OR started_at >= created_at),
    CHECK (
        extraction_open_at IS NULL
        OR (started_at IS NOT NULL AND extraction_open_at >= started_at)
    ),
    CHECK (
        hard_deadline IS NULL
        OR (extraction_open_at IS NOT NULL AND hard_deadline >= extraction_open_at)
    ),
    CHECK (
        settlement_grace_deadline IS NULL
        OR (hard_deadline IS NOT NULL AND settlement_grace_deadline >= hard_deadline)
    ),
    CHECK (finished_at IS NULL OR finished_at >= created_at)
);

CREATE INDEX matches_nonterminal_state_idx
    ON matches (state, created_at)
    WHERE state NOT IN ('finished', 'aborted');

CREATE TABLE match_participants (
    match_id UUID NOT NULL REFERENCES matches(id),
    account_id UUID NOT NULL REFERENCES accounts(id),
    public_player_id UUID NOT NULL,
    seat_id SMALLINT NOT NULL CHECK (seat_id BETWEEN 0 AND 9),
    state TEXT NOT NULL CHECK (
        state IN ('waiting', 'preparing', 'active', 'disconnected', 'settlement_pending', 'dead', 'extracted', 'timed_out', 'aborted')
    ),
    reconnect_deadline TIMESTAMPTZ,
    killed_by_account_id UUID,
    extracted_at TIMESTAMPTZ,
    settlement_qualified_at TIMESTAMPTZ,
    mined_counts JSONB NOT NULL DEFAULT '{}'::JSONB CHECK (jsonb_typeof(mined_counts) = 'object'),
    pickup_counts JSONB NOT NULL DEFAULT '{}'::JSONB CHECK (jsonb_typeof(pickup_counts) = 'object'),
    lost_counts JSONB NOT NULL DEFAULT '{}'::JSONB CHECK (jsonb_typeof(lost_counts) = 'object'),
    PRIMARY KEY (match_id, account_id),
    UNIQUE (match_id, seat_id),
    UNIQUE (match_id, public_player_id),
    FOREIGN KEY (match_id, killed_by_account_id)
        REFERENCES match_participants(match_id, account_id)
);

CREATE UNIQUE INDEX match_participants_active_account_idx
    ON match_participants (account_id)
    WHERE state IN ('waiting', 'preparing', 'active', 'disconnected', 'settlement_pending');
CREATE INDEX match_participants_match_state_idx
    ON match_participants (match_id, state);

CREATE TABLE extraction_settlements (
    id UUID PRIMARY KEY,
    match_id UUID NOT NULL,
    account_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) > 0),
    inventory_digest BYTEA NOT NULL CHECK (octet_length(inventory_digest) = 32),
    config_version TEXT NOT NULL CHECK (length(config_version) > 0),
    total_value BIGINT NOT NULL CHECK (total_value >= 0),
    committed_at TIMESTAMPTZ NOT NULL,
    UNIQUE (match_id, account_id),
    UNIQUE (id, account_id),
    FOREIGN KEY (match_id, account_id)
        REFERENCES match_participants(match_id, account_id)
);

CREATE TABLE settlement_items (
    settlement_id UUID NOT NULL REFERENCES extraction_settlements(id),
    item_key TEXT NOT NULL CHECK (item_key IN ('dirt', 'gold', 'diamond')),
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    PRIMARY KEY (settlement_id, item_key)
);

CREATE TABLE warehouse_balances (
    account_id UUID NOT NULL REFERENCES accounts(id),
    item_key TEXT NOT NULL CHECK (item_key IN ('dirt', 'gold', 'diamond')),
    quantity BIGINT NOT NULL CHECK (quantity >= 0),
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (account_id, item_key)
);

CREATE TABLE asset_ledger (
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts(id),
    settlement_id UUID NOT NULL,
    item_key TEXT NOT NULL CHECK (item_key IN ('dirt', 'gold', 'diamond')),
    delta BIGINT NOT NULL CHECK (delta > 0),
    created_at TIMESTAMPTZ NOT NULL,
    UNIQUE (settlement_id, item_key),
    FOREIGN KEY (settlement_id, account_id)
        REFERENCES extraction_settlements(id, account_id),
    FOREIGN KEY (settlement_id, item_key)
        REFERENCES settlement_items(settlement_id, item_key)
);

CREATE INDEX extraction_settlements_account_idx
    ON extraction_settlements (account_id, committed_at DESC);
CREATE INDEX asset_ledger_account_idx
    ON asset_ledger (account_id, created_at DESC);
