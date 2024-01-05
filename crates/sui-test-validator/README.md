The sui-test-validator starts a local network that includes a Sui Full node, a Sui validator, a Sui faucet and (optionally)
an indexer.

## Guide

Refer to [sui-local-network.md](../../doc/src/build/sui-local-network.md)

## Experimental Feature - Running with Indexer

**Note** Similar to the fullnode db, all state will be wiped upon restart

1. Follow the [Prerequisites section](../../crates/sui-indexer/README.md#prerequisites) in the `sui-indexer` README to set up the postgresdb on your local machine
2. Make sure the `Posgresdb` starts on your local machine
3. run `RUST_LOG="consensus=off" ./target/debug/sui-test-validator --with-indexer`
4. To check your local db, if you use the default db url `postgres://postgres:postgres@localhost:5432/sui_indexer`, you can login to the `postgres` database and run `\dt` to show all tables.
