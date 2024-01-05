// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::ops::Neg;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};

use sui_types::base_types::{ObjectID, ObjectRef, SuiAddress};
use sui_types::coin::{COIN_JOIN_FUNC_NAME, COIN_MODULE_NAME, COIN_SPLIT_VEC_FUNC_NAME};
use sui_types::event::{Event, TransferType};
use sui_types::gas_coin::GasCoin;
use sui_types::messages::{
    CallArg, InputObjectKind, MoveCall, ObjectArg, Pay, SingleTransactionKind, TransactionData,
    TransactionEffects,
};
use sui_types::move_package::disassemble_modules;
use sui_types::{parse_sui_struct_tag, SUI_FRAMEWORK_OBJECT_ID};

use crate::types::{
    AccountIdentifier, Amount, CoinAction, CoinChange, CoinID, CoinIdentifier,
    ConstructionMetadata, IndexCounter, OperationIdentifier, OperationStatus, OperationType,
    SignedValue,
};
use crate::ErrorType::UnsupportedOperation;
use crate::{Error, ErrorType, SUI};

#[cfg(test)]
#[path = "unit_tests/operations_tests.rs"]
mod operations_tests;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Operation {
    pub operation_identifier: OperationIdentifier,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_operations: Vec<OperationIdentifier>,
    #[serde(rename = "type")]
    pub type_: OperationType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<OperationStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountIdentifier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<Amount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coin_change: Option<CoinChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl Operation {
    pub fn from_data(data: &TransactionData) -> Result<Vec<Operation>, anyhow::Error> {
        let budget = data.gas_budget;
        let gas = data.gas();
        let sender = data.signer();
        Ok(data
            .kind
            .single_transactions()
            .flat_map(|tx| {
                parse_operations(
                    tx,
                    budget,
                    gas,
                    sender,
                    &mut IndexCounter::default(),
                    None,
                    None,
                    &[],
                )
            })
            .flatten()
            .collect::<Vec<_>>())
    }

    pub fn from_data_and_effect(
        data: &TransactionData,
        effects: &TransactionEffects,
        new_coins: &[(GasCoin, ObjectRef)],
    ) -> Result<Vec<Operation>, anyhow::Error> {
        let budget = data.gas_budget;
        let gas = data.gas();
        let sender = data.signer();
        let mut counter = IndexCounter::default();
        let status = Some((&effects.status).into());
        let mut operations = data
            .kind
            .single_transactions()
            .flat_map(|tx| {
                parse_operations(
                    tx,
                    budget,
                    gas,
                    sender,
                    &mut counter,
                    status,
                    Some(effects),
                    new_coins,
                )
            })
            .flatten()
            .collect::<Vec<_>>();

        operations.push(Operation {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::GasSpent,
            // We always charge gas
            status: Some(OperationStatus::Success),
            account: Some(AccountIdentifier { address: sender }),
            amount: Some(Amount {
                value: effects.gas_used.net_gas_usage().neg().into(),
                currency: SUI.clone(),
            }),
            coin_change: None,
            metadata: None,
        });

        Ok(operations)
    }

    fn get_coin_operation_from_events(
        input_objects: &[InputObjectKind],
        new_coins: &[(GasCoin, ObjectRef)],
        events: &[Event],
        status: Option<OperationStatus>,
        counter: &mut IndexCounter,
    ) -> Vec<Operation> {
        events
            .iter()
            .flat_map(|event| {
                Self::get_coin_operation_from_event(
                    input_objects,
                    new_coins,
                    event,
                    status,
                    counter,
                )
            })
            .collect()
    }

    fn get_coin_operation_from_event(
        input_objects: &[InputObjectKind],
        new_coins: &[(GasCoin, ObjectRef)],
        event: &Event,
        status: Option<OperationStatus>,
        counter: &mut IndexCounter,
    ) -> Vec<Operation> {
        match event {
            Event::TransferObject {
                sender,
                recipient,
                object_id,
                version,
                type_: TransferType::Coin,
                amount: Some(amount),
                ..
            } => {
                let input = input_objects.iter().find_map(|kind| {
                    if let InputObjectKind::ImmOrOwnedMoveObject((id, version, _)) = kind {
                        if id == object_id {
                            return Some(CoinChange {
                                coin_identifier: CoinIdentifier {
                                    identifier: CoinID {
                                        id: *id,
                                        version: *version,
                                    },
                                },
                                coin_action: CoinAction::CoinSpent,
                            });
                        }
                    }
                    None
                });
                vec![
                    Operation {
                        operation_identifier: counter.next_idx().into(),
                        related_operations: vec![],
                        type_: OperationType::TransferSUI,
                        status,
                        account: Some(AccountIdentifier { address: *sender }),
                        amount: Some(Amount {
                            value: SignedValue::neg((*amount).try_into().unwrap()),
                            currency: SUI.clone(),
                        }),
                        coin_change: input,
                        metadata: None,
                    },
                    Operation {
                        operation_identifier: counter.next_idx().into(),
                        related_operations: vec![],
                        type_: OperationType::TransferSUI,
                        status,
                        account: recipient.get_owner_address().ok().map(|addr| addr.into()),
                        amount: Some(Amount {
                            value: (*amount).into(),
                            currency: SUI.clone(),
                        }),
                        coin_change: Some(CoinChange {
                            coin_identifier: CoinIdentifier {
                                identifier: CoinID {
                                    id: *object_id,
                                    version: *version,
                                },
                            },
                            coin_action: CoinAction::CoinCreated,
                        }),
                        metadata: None,
                    },
                ]
            }
            Event::NewObject {
                package_id: _,
                transaction_module: _,
                sender: _,
                recipient,
                object_id,
            } => {
                if let Some((coin, (id, version, _))) =
                    new_coins.iter().find(|(_, (id, _, _))| id == object_id)
                {
                    let amount = coin.value();
                    vec![Operation {
                        operation_identifier: counter.next_idx().into(),
                        related_operations: vec![],
                        type_: OperationType::TransferSUI,
                        status,
                        account: recipient.get_owner_address().ok().map(|addr| addr.into()),
                        amount: Some(Amount {
                            value: amount.into(),
                            currency: SUI.clone(),
                        }),
                        coin_change: Some(CoinChange {
                            coin_identifier: CoinIdentifier {
                                identifier: CoinID {
                                    id: *id,
                                    version: *version,
                                },
                            },
                            coin_action: CoinAction::CoinCreated,
                        }),
                        metadata: None,
                    }]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }

    pub async fn parse_transaction_data(
        operations: Vec<Operation>,
        metadata: ConstructionMetadata,
    ) -> Result<TransactionData, Error> {
        let action: SuiAction = operations.try_into()?;
        action.try_into_data(metadata).await
    }

    pub fn gas_budget(
        counter: &mut IndexCounter,
        status: Option<OperationStatus>,
        gas: ObjectRef,
        budget: u64,
        sender: SuiAddress,
    ) -> Self {
        Self {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::GasBudget,
            status,
            account: Some(AccountIdentifier { address: sender }),
            amount: None,
            coin_change: Some(CoinChange {
                coin_identifier: CoinIdentifier {
                    identifier: gas.into(),
                },
                coin_action: CoinAction::CoinSpent,
            }),
            metadata: Some(json!({ "budget": budget })),
        }
    }
}

fn parse_operations(
    tx: &SingleTransactionKind,
    budget: u64,
    gas: ObjectRef,
    sender: SuiAddress,
    counter: &mut IndexCounter,
    status: Option<OperationStatus>,
    effects: Option<&TransactionEffects>,
    new_coins: &[(GasCoin, ObjectRef)],
) -> Result<Vec<Operation>, anyhow::Error> {
    let mut operations = match tx {
        SingleTransactionKind::TransferSui(tx) => transfer_sui_operations(
            budget,
            gas,
            sender,
            tx.recipient,
            tx.amount,
            counter,
            status,
        ),
        SingleTransactionKind::TransferObject(tx) => transfer_object_operations(
            budget,
            tx.object_ref,
            gas,
            sender,
            tx.recipient,
            counter,
            status,
        ),
        SingleTransactionKind::Call(c) => {
            move_call_operations(sender, gas, budget, c, counter, status)
        }
        SingleTransactionKind::Publish(p) => {
            let disassembled = disassemble_modules(p.modules.iter())?;
            vec![Operation {
                operation_identifier: counter.next_idx().into(),
                related_operations: vec![],
                type_: OperationType::Publish,
                status,
                account: Some(AccountIdentifier { address: sender }),
                amount: None,
                coin_change: None,
                metadata: Some(json!(disassembled)),
            }]
        }
        SingleTransactionKind::ChangeEpoch(change) => vec![Operation {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::EpochChange,
            status,
            account: None,
            amount: None,
            coin_change: None,
            metadata: Some(json!(change)),
        }],
        SingleTransactionKind::Pay(pay) => parse_pay(sender, gas, budget, pay, counter, status),
    };
    if !matches!(tx, SingleTransactionKind::TransferSui(..)) {
        if let Some(effects) = effects {
            let coin_change_operations = Operation::get_coin_operation_from_events(
                &tx.input_objects()?,
                new_coins,
                &effects.events,
                status,
                counter,
            );
            operations.extend(coin_change_operations);
        }
    }
    Ok(operations)
}

fn transfer_sui_operations(
    budget: u64,
    coin: ObjectRef,
    sender: SuiAddress,
    recipient: SuiAddress,
    amount: Option<u64>,
    counter: &mut IndexCounter,
    status: Option<OperationStatus>,
) -> Vec<Operation> {
    vec![
        Operation {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::TransferSUI,
            status,
            account: Some(AccountIdentifier { address: sender }),
            amount: amount.map(|amount| Amount {
                value: SignedValue::neg(amount.try_into().unwrap()),
                currency: SUI.clone(),
            }),
            coin_change: Some(CoinChange {
                coin_identifier: CoinIdentifier {
                    identifier: coin.into(),
                },
                coin_action: CoinAction::CoinSpent,
            }),
            metadata: None,
        },
        Operation {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::TransferSUI,
            status,
            account: Some(AccountIdentifier { address: recipient }),
            amount: amount.map(|amount| Amount {
                value: amount.into(),
                currency: SUI.clone(),
            }),
            coin_change: None,
            metadata: None,
        },
        Operation::gas_budget(counter, status, coin, budget, sender),
    ]
}

fn transfer_object_operations(
    budget: u64,
    object_id: ObjectRef,
    gas: ObjectRef,
    sender: SuiAddress,
    recipient: SuiAddress,
    counter: &mut IndexCounter,
    status: Option<OperationStatus>,
) -> Vec<Operation> {
    vec![
        Operation {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::TransferObject,
            status,
            account: Some(AccountIdentifier { address: sender }),
            amount: None,
            coin_change: None,
            metadata: Some(json!({ "object_id": object_id.0, "version": object_id.1 })),
        },
        Operation {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::TransferObject,
            status,
            account: Some(AccountIdentifier { address: recipient }),
            amount: None,
            coin_change: None,
            metadata: None,
        },
        Operation::gas_budget(counter, status, gas, budget, sender),
    ]
}

fn move_call_operations(
    sender: SuiAddress,
    gas: ObjectRef,
    budget: u64,
    call: &MoveCall,
    counter: &mut IndexCounter,
    status: Option<OperationStatus>,
) -> Vec<Operation> {
    vec![
        Operation {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::MoveCall,
            status,
            account: Some(AccountIdentifier { address: sender }),
            amount: None,
            coin_change: None,
            metadata: Some(json!(call)),
        },
        Operation::gas_budget(counter, status, gas, budget, sender),
    ]
}

fn parse_pay(
    sender: SuiAddress,
    gas: ObjectRef,
    budget: u64,
    pay: &Pay,
    counter: &mut IndexCounter,
    status: Option<OperationStatus>,
) -> Vec<Operation> {
    vec![
        Operation {
            operation_identifier: counter.next_idx().into(),
            related_operations: vec![],
            type_: OperationType::Pay,
            status,
            account: Some(AccountIdentifier { address: sender }),
            amount: None,
            coin_change: None,
            metadata: Some(json!(pay)),
        },
        Operation::gas_budget(counter, status, gas, budget, sender),
    ]
}

#[derive(Debug)]
pub enum SuiAction {
    TransferSui {
        budget: u64,
        coin: ObjectID,
        sender: SuiAddress,
        recipient: SuiAddress,
        amount: Option<u64>,
    },

    Transfer {
        budget: u64,
        coin: ObjectID,
        gas: ObjectID,
        sender: SuiAddress,
        recipient: SuiAddress,
    },

    MergeCoin {
        budget: u64,
        primary_coin: ObjectID,
        coin_to_merge: ObjectID,
        gas: ObjectID,
        sender: SuiAddress,
    },
    SplitCoin {
        budget: u64,
        coin_to_split: ObjectID,
        split_amounts: Vec<u64>,
        gas: ObjectID,
        sender: SuiAddress,
    },
}

impl SuiAction {
    pub async fn try_into_data(
        self,
        metadata: ConstructionMetadata,
    ) -> Result<TransactionData, Error> {
        Ok(match self {
            SuiAction::TransferSui {
                budget,
                coin,
                sender,
                recipient,
                amount,
            } => {
                let gas = metadata.try_get_info(&coin)?;
                TransactionData::new_transfer_sui(recipient, sender, amount, gas.into(), budget)
            }
            SuiAction::Transfer {
                budget,
                coin,
                gas,
                sender,
                recipient,
            } => {
                let gas = metadata.try_get_info(&gas)?;
                let coin = metadata.try_get_info(&coin)?;
                TransactionData::new_transfer(recipient, coin.into(), sender, gas.into(), budget)
            }
            SuiAction::MergeCoin {
                budget,
                primary_coin,
                coin_to_merge,
                gas,
                sender,
            } => {
                let gas = metadata.try_get_info(&gas)?;
                let primary_coin = metadata.try_get_info(&primary_coin)?;
                let coin_to_merge = metadata.try_get_info(&coin_to_merge)?;
                let type_args = parse_sui_struct_tag(&primary_coin.type_)?.type_params;

                TransactionData::new_move_call(
                    sender,
                    metadata.try_get_info(&SUI_FRAMEWORK_OBJECT_ID)?.into(),
                    COIN_MODULE_NAME.to_owned(),
                    COIN_JOIN_FUNC_NAME.to_owned(),
                    type_args,
                    gas.into(),
                    vec![
                        CallArg::Object(ObjectArg::ImmOrOwnedObject(primary_coin.into())),
                        CallArg::Object(ObjectArg::ImmOrOwnedObject(coin_to_merge.into())),
                    ],
                    budget,
                )
            }
            SuiAction::SplitCoin {
                budget,
                coin_to_split,
                split_amounts,
                gas,
                sender,
            } => {
                let gas = metadata.try_get_info(&gas)?;
                let coin_to_split = metadata.try_get_info(&coin_to_split)?;
                let type_args = parse_sui_struct_tag(&coin_to_split.type_)?.type_params;
                TransactionData::new_move_call(
                    sender,
                    metadata.try_get_info(&SUI_FRAMEWORK_OBJECT_ID)?.into(),
                    COIN_MODULE_NAME.to_owned(),
                    COIN_SPLIT_VEC_FUNC_NAME.to_owned(),
                    type_args,
                    gas.into(),
                    vec![
                        CallArg::Object(ObjectArg::ImmOrOwnedObject(coin_to_split.into())),
                        CallArg::Pure(bcs::to_bytes(&split_amounts)?),
                    ],
                    budget,
                )
            }
        })
    }

    pub fn input_objects(&self) -> Vec<ObjectID> {
        match self {
            SuiAction::TransferSui { coin, .. } => {
                vec![*coin]
            }
            SuiAction::Transfer { coin, gas, .. } => vec![*coin, *gas],
            SuiAction::MergeCoin {
                primary_coin,
                coin_to_merge,
                gas,
                ..
            } => vec![SUI_FRAMEWORK_OBJECT_ID, *primary_coin, *coin_to_merge, *gas],
            SuiAction::SplitCoin {
                coin_to_split, gas, ..
            } => vec![SUI_FRAMEWORK_OBJECT_ID, *coin_to_split, *gas],
        }
    }

    pub fn signer(&self) -> SuiAddress {
        *match self {
            SuiAction::TransferSui { sender, .. }
            | SuiAction::Transfer { sender, .. }
            | SuiAction::MergeCoin { sender, .. }
            | SuiAction::SplitCoin { sender, .. } => sender,
        }
    }
}

impl TryInto<SuiAction> for Vec<Operation> {
    type Error = Error;

    fn try_into(self) -> Result<SuiAction, Self::Error> {
        let mut builder = SuiActionBuilder::default();

        for op in self {
            match op.type_ {
                OperationType::TransferSUI => {
                    let account = op
                        .account
                        .as_ref()
                        .ok_or_else(|| Error::missing_input("operation.account"))?;
                    let address = account.address;
                    builder.operation_type = Some(op.type_);
                    if let Some(amount) = op.amount.as_ref() {
                        if amount.value.is_negative() {
                            builder.sender = Some(address);
                            builder.send_amount = Some(amount.value.abs().try_into()?);
                            if let Some(coin) = op.coin_change.as_ref() {
                                builder.gas = Some(coin.coin_identifier.identifier.id);
                            }
                        } else {
                            builder.recipient = Some(address);
                        }
                    } else if let Some(coin) = op.coin_change.as_ref() {
                        // no amount specified, sending the whole coin
                        builder.sender = Some(address);
                        builder.coin = Some(coin.coin_identifier.identifier.id);
                    } else {
                        builder.recipient = Some(address);
                    }
                }
                OperationType::GasBudget => {
                    if let Some(coin) = op.coin_change.as_ref() {
                        builder.gas = Some(coin.coin_identifier.identifier.id);
                    }
                    let budget_value = op
                        .metadata
                        .and_then(|v| v.pointer("/budget").cloned())
                        .ok_or_else(|| Error::missing_input("gas budget"))?;

                    let budget = budget_value
                        .as_u64()
                        .or_else(|| budget_value.as_str().and_then(|s| u64::from_str(s).ok()))
                        .ok_or_else(|| {
                            Error::new_with_msg(
                                ErrorType::InvalidInput,
                                format!("Cannot parse gas budget : [{budget_value}]").as_str(),
                            )
                        })?;
                    builder.gas_budget = Some(budget);
                }
                OperationType::TransferObject
                | OperationType::Pay
                | OperationType::GasSpent
                | OperationType::Genesis
                | OperationType::MoveCall
                | OperationType::Publish
                | OperationType::EpochChange => return Err(Error::unsupported_operation(op.type_)),
            }
        }
        builder.build()
    }
}

#[derive(Default)]
struct SuiActionBuilder {
    sender: Option<SuiAddress>,
    recipient: Option<SuiAddress>,
    gas: Option<ObjectID>,
    coin: Option<ObjectID>,
    send_amount: Option<u64>,
    gas_budget: Option<u64>,
    operation_type: Option<OperationType>,
}

impl SuiActionBuilder {
    fn build(self) -> Result<SuiAction, Error> {
        let type_ = self
            .operation_type
            .ok_or_else(|| Error::missing_input("operation_type"))?;
        match type_ {
            OperationType::TransferSUI => {
                let sender = self.sender.ok_or_else(|| Error::missing_input("sender"))?;
                let recipient = self
                    .recipient
                    .ok_or_else(|| Error::missing_input("recipient"))?;
                let gas = self.gas.ok_or_else(|| Error::missing_input("gas"))?;
                let budget = self
                    .gas_budget
                    .ok_or_else(|| Error::missing_input("gas_budget"))?;
                Ok(SuiAction::TransferSui {
                    budget,
                    coin: gas,
                    sender,
                    recipient,
                    amount: self.send_amount,
                })
            }
            _ => Err(Error::new_with_msg(
                UnsupportedOperation,
                format!("Unsupported operation [{type_:?}]").as_str(),
            )),
        }
    }
}
