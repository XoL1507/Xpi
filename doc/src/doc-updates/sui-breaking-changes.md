---
title: Sui Breaking Changes in Release .28
---

The next release of Sui, release 0.28, includes the breaking changes described in this topic. A breaking change is one that introduces new, or changed, Sui functionality that causes existing apps and implementations to stop functioning as expected. Updates to this topic for next week include information about how to resolve or update your project that might break as a result of these changes.

**[Major breaking change]** - The new Programmable Transactions feature introduces a new type of transaction that replaces both batch transactions and normal transactions (with the exception of special system transactions). These transactions allow for a series of Commands (mini transactions of sorts) to be executed, where the results of commands can be used in following commands. For more information, see the [Programmable Transactions RFC](https://forums.sui.io/t/rfc-planned-feature-programmable-transactions/3823).

**[Major breaking change]** - `SuiAddress` and `ObjectID` are now 32 bytes long instead of 20 bytes (in hex, the `len` increases from 40 to 64). If your software interacts with any `ObjectID` and `SuiAddress`, you must update it to use updated addresses and redeploy it. [PR 8542](https://github.com/MystenLabs/sui/pull/8542)

**[Breaking change]** - Changes to Gas Budget to use SUI rather than gas units. This removes the concept of gas units from any user-related API operations. This does not change the format of `TransactionData` (u64). This is not a breaking change in the sense that the current format no longer works, but rather requires you to reconsider how you use gas budgets.

**[Breaking change]** - Currently, transactions require a single coin to pay for gas. This sometimes results in users needing to make separate transactions (such as `PaySui`) to merge coins before completing a transaction, which can also increase the cost of the transaction.

This release changes the field value type in `TransactionData` from `gas_payment: ObjectRef` to `gas_payment: Vec<ObjectRef>`, where `Vec<ObjectRef>` is a non-empty vector of owned SUI objects. This combines all of the coins into a single coin, using the `ObjectID` of the first coin in the vector as the coin containing the merge.

**[Breaking change]** - `ecdsa_k1::ecrecover` and `ecdsa_k1::secp256k1_verify` now require you to input the raw message instead of a hashed message. You must also include the u8 that represents  the hash function. See [PR 7773](https://github.com/MystenLabs/sui/pull/7773) for more details.

**[Breaking change]** The `ValidatorMetadata` function now includes a p2p_address field. The value for the field is the address the validator used for p2p activities, such as state sync. To learn more, see [PR 8636](https://github.com/MystenLabs/sui/pull/8636).

**[Transaction Format breaking change]** - Adds a new expiration field to `TransactionData` to allow for users to specify a time that a transaction should expire, meaning it is no longer eligible to sign and execute by validators. In this release, the only supported value for the expiration field is epoch`. If not provided, no expiration is set for the associated transaction.

**[Minor breaking change]** - This release modifies the format for `ConsensusCommitPrologue` transactions. This is a system-generated transaction that updates timestamp on the `Clock` object, allowing Sui Move smart contracts to read up-to-date timestamps from the blockchain.

**[Minor breaking change]** - Removes `bulletproofs` and `elliptic_curve` modules from the Sui Framework. For more information, see [PR 8660](https://github.com/MystenLabs/sui/pull/8660)

**[API breaking change]** - This release replaces `SuiCertifiedTransaction` with `SuiTransaction` in `SuiTransactionResponse`. This is because validators can no longer guarantee to return a transaction certificate. This release also unifies `SuiTransactionResponse` and `SuiExecuteTransactionResponse` to simplify the API. See [PR 8369](https://github.com/MystenLabs/sui/pull/8369) for more information.

**[API breaking change]** - Updates the structure for dynamic field names to make it easier to use in `sui_getDynamicFieldObject`. For more details, see [PR 7318](https://github.com/MystenLabs/sui/pull/7318)

**[API breaking change]** - This release removes the request_switch_delegation function from the Transaction Builder API. It also removes the pending_delegation_switches field from the validator set type in the Sui SDK. See [PR 8435](https://github.com/MystenLabs/sui/pull/8435) for more information.

**[API breaking change]** - To reduce the size of Sui Full node synchronization payloads, this release removes events from `TransactionEffect`. The events are still included in the `SuiTransactionResponse` returned by `sui_getTransaction` and `sui_submitTransaction` endpoints. For more information, see [PR 7822](https://github.com/MystenLabs/sui/pull/7822).