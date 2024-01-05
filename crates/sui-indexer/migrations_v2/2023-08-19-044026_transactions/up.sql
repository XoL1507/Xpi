CREATE TABLE transactions (
    tx_sequence_number          BIGINT       PRIMARY KEY,
    transaction_digest          bytea        NOT NULL,
    -- bcs serialized SenderSignedData bytes
    raw_transaction             bytea        NOT NULL,
    -- bcs serialized TransactionEffects bytes
    raw_effects                 bytea        NOT NULL,
    checkpoint_sequence_number  BIGINT       NOT NULL,
    timestamp_ms                BIGINT       NOT NULL,
    -- array of bcs serialized IndexedObjectChange bytes
    object_changes              bytea[]      NOT NULL,
    -- array of bcs serialized BalanceChange bytes
    balance_changes             bytea[]      NOT NULL,
    -- array of bcs serialized StoredEvent bytes
    events                      bytea[]      NOT NULL,
    -- SystemTransaction/ProgrammableTransaction. See types_v2.rs
    transaction_kind            smallint     NOT NULL
);

CREATE INDEX transactions_transaction_digest ON transactions (transaction_digest);
CREATE INDEX transactions_checkpoint_sequence_number ON transactions (checkpoint_sequence_number);

-- only create index for system transactions (0). See types_v2.rs
CREATE INDEX transactions_transaction_kind ON transactions (transaction_kind) WHERE transaction_kind <> 0;
