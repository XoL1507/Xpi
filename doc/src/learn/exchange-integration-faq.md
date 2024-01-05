---
title: Sui Exchange Integration FAQ
---

The Sui blockchain is still in its development stages, therefore many of the solutions provided in this topic are interim. Please do not hesitate to contact us if you are having issues with the information provided.

## Where are the Sui Developer Docs?

- Sui Documentation Portal: [https://docs.sui.io/](https://docs.sui.io/)
- Sui REST API's: [https://docs.sui.io/sui-jsonrpc](https://docs.sui.io/sui-jsonrpc)
- Run a Full node: [https://docs.sui.io/devnet/build/fullnode](https://docs.sui.io/devnet/build/fullnode)

## When will Testnet be live?

Testnet Wave 1 ended on 12/01/22. Information about subsequent Testnet waves will be provided when available.

## How will SUI staking work?

Sample staking implementation:

The entry functions for staking are defined in [this module](https://github.com/MystenLabs/sui/blob/main/crates/sui-framework/sources/governance/sui_system.move). Relevant functions include:
 * [`request_add_stake`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_add_stake)
 * [`request_add_stake_with_locked_coin`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_add_stake_with_locked_coin)
 * [`request_withdraw_stake`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_withdraw_stake)
 * [`request_add_delegation`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_add_delegation)
 * [`request_add_delegation_with_locked_coin`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_add_delegation_with_locked_coin)
 * [`request_withdraw_delegation`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_withdraw_delegation)
 * [`request_switch_delegation`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_switch_delegation)

The first three functions are for validator staking for themselves while the rest are for delegated staking.

### How many validators will Sui have at genesis?

The number is still under consideration. The validator set is not fixed, but validators must apply and then be approved through our validator application process.

### Is the address used for staking the same as the wallet address that owns the staked coins?

Yes, a user/validator stakes using the address that owns the staked coin. There is no special address derivation

### How is a staking transaction different from a typical transaction regarding construction, signing, and broadcasting?

Staking transactions are Move call transactions that call specific Move function in the [Sui Framework](https://github.com/MystenLabs/sui/blob/main/crates/sui-framework/sources/governance/sui_system.move). The staking transaction uses a shared object, and is no different from other shared object transactions.

### Does Sui support staking a partial amount of the SUI owned by an address?

Yes, an address can own multiple coins of different amounts. Sui supports staking coins owned by an address to different validators. The minimum staking amount that can be delegated is 1 MIST which is equal to .000000001 SUI.

### Can I use one account address to stake with multiple validators?

Yes, if an address owns multiple coins, you can stake each coin with a different validator.

### Can I change the amount of an existing stake during the staking period?

Yes, you can add to or withdraw your stake from a validator. Use the following methods to modify the stake amount:

Use the [`request_add_delegation`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_add_delegation) and [`request_add_delegation_with_locked_coin`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_add_delegation_with_locked_coin) methods to add to the staked amount.

Use the [`request_withdraw_delegation`](https://github.com/MystenLabs/sui/blob/58229627970a6e9ff558b156c1cb193f246eaf88/crates/sui-framework/docs/sui_system.md#0x2_sui_system_request_withdraw_delegation) method to withdraw all or part of the delegation.

### Can I change the validator while a coin is actively staked?

Yes, use the `request_switch_delegation` method to change the validator while a coin is staked. Examples coming soon.

### Does Sui require a bonding / warm-up period?

Yes, but the specifics are still under consideration. We expect a period of only a few days maximum.

### Does Sui require an un-bonding period?

The current un-bonding period is one week, this may change prior to Mainnet launch.

### Are staking rewards auto-compounded?

Yes, Sui uses a staking pool approach inspired by liquidity pools. Rewards are added to the pool and auto-compounded through the appreciation of pool token value relative to SUI tokens.

### Do rewards appear as inbound/outbound on-chain transactions?

Yes, rewards are added to the staking pool through a special system transaction at epoch boundaries.

### How long does it take to get the first reward after staking? How frequently are rewards paid out?

Rewards are compounded every epoch, and paid out when you withdraw your stake. You must stake for the entire duration of an epoch to receive rewards for that epoch.

### Is there a minimum and maximum staking amount (for validation and delegation)?

There will be a minimum amount required and a maximum amount allowed, as well as limits on stake changes within an epoch.

 * Validation: Requires a high minimum amount of SUI.
 * Delegation: No minimum

Specific amounts will be provided when available.

### How does slashing work, and what are the penalties?

There will not be slashing for the principal stake allocated. Instead, validators will get penalized by having fewer future rewards when these get paid out. Rewards that have already been accrued are not at risk.

### Does Sui support on-chain governance or voting?

On-chain governance is not implemented for Sui. There is no plan to add it in the near future.

## Where can I find block details?

Answers to the following questions address specific details around surfacing block details.

### How can I retrieve the current block height or query a block by height using a Sui endpoint?

Sui is [DAG](https://cointelegraph.com/explained/what-is-a-directed-acyclic-graph-in-cryptocurrency-how-does-dag-work)-based, so the block-based view of the transaction history is not always the most direct one. To get the latest transaction, use the Transaction Query API:

    ```json
    {
      "jsonrpc": "2.0",
      "id": 1,
      "method": "sui_getTransactions",
      "params": [
        "All",
        <last known transaction digest>,
        100,
        "Ascending"
      ]
    }
    ```

### How do I query for balance changes?

The following solution is interim:
Use the `BalanceChangeEvent` using the event query API. `BalanceChangeEvent` was added in October 2022 in this [PR](https://github.com/MystenLabs/sui/pull/5289).

### How do I track block generation?

Sui uses checkpoints, but this is still under development. Checkpoints are like blocks that are created periodically (likely every few minutes), but generated asynchronously instead of on the critical path for execution. Each checkpoint contains all of the confirmed transactions since the previous checkpoint.

A significant amount of the performance benefits of Sui comes from carefully separating the work required to actually finalize a transaction from bookkeeping work, like checkpoint construction. We're experimenting with a number of different generation intervals, and this may change over time as we better understand the traffic patterns.

**Temporary solution** For now, we have created one block per transaction until Checkpoints become available. See sample [here](https://github.com/MystenLabs/sui/blob/91a5e988a91b41d920a082f3de3c2c7372627b00/crates/sui-rosetta/src/state.rs#L61-L74).

    ```rust
    #[async_trait]
    pub trait BlockProvider {
        async fn get_block_by_index(&self, index: u64) -> Result<BlockResponse, Error>;
        async fn get_block_by_hash(&self, hash: BlockHash) -> Result<BlockResponse, Error>;
        async fn current_block(&self) -> Result<BlockResponse, Error>;
        fn genesis_block_identifier(&self) -> BlockIdentifier;
        async fn oldest_block_identifier(&self) -> Result<BlockIdentifier, Error>;
        async fn current_block_identifier(&self) -> Result<BlockIdentifier, Error>;
        async fn get_balance_at_block(
            &self,
            addr: SuiAddress,
            block_height: u64,
        ) -> Result<u128, Error>;
    }
    ```

### How are transactions proposed by validators if they're not included in blocks? Does a validator propose blocks or just individual transactions?

Validators form a certificate (a quorum of signatures) for each transaction, and then propose checkpoints consisting of certificates since the last checkpoint. You can read more in section 4.3 [here](https://github.com/MystenLabs/sui/blob/main/doc/paper/sui.pdf).

### How do I get Devnet coins?

- You can find our [faucet in Discord](https://discord.com/channels/916379725201563759/971488439931392130).

### How can I get in touch and request more information?

- Please visit our [Discord server](https://discord.gg/sui).
