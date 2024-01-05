// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::bail;
use sui_core::gateway_state::GatewayClient;
use sui_json_rpc_types::{GetObjectDataResponse, SuiEvent, SuiObject, SuiParsedMoveObject};
use sui_types::gas_coin::GasCoin;
use sui_types::{
    base_types::{ObjectID, SequenceNumber, SuiAddress},
    event::TransferType,
    object::Owner,
};

use tracing::debug;

/// A util struct that helps verify Sui Object.
/// Use builder style to construct the conditions.
/// When optionals fields are not set, related checks are omitted.
/// Consuming functions such as `check` perform the check and panics if
/// verification results are unexpected. `check_into_sui_object` and
/// `check_info_gas_object` expect to get a `SuiObject` and `GasObject`
/// respectfully.
/// ```
#[derive(Debug)]
pub struct ObjectChecker {
    object_id: ObjectID,
    owner: Option<Owner>,
    is_deleted: bool,
    is_sui_coin: Option<bool>,
}

impl ObjectChecker {
    pub fn new(object_id: ObjectID) -> ObjectChecker {
        Self {
            object_id,
            owner: None,
            is_deleted: false, // default to exist
            is_sui_coin: None,
        }
    }

    pub fn owner(mut self, owner: Owner) -> Self {
        self.owner = Some(owner);
        self
    }

    pub fn deleted(mut self) -> Self {
        self.is_deleted = true;
        self
    }

    pub fn is_sui_coin(mut self, is_sui_coin: bool) -> Self {
        self.is_sui_coin = Some(is_sui_coin);
        self
    }

    pub async fn check_into_gas_coin(self, client: &GatewayClient) -> GasCoin {
        if self.is_sui_coin == Some(false) {
            panic!("'check_into_gas_coin' shouldn't be called with 'is_sui_coin' set as false");
        }
        self.is_sui_coin(true)
            .check(client)
            .await
            .unwrap()
            .into_gas_coin()
    }

    pub async fn check_into_sui_object(
        self,
        client: &GatewayClient,
    ) -> SuiObject<SuiParsedMoveObject> {
        self.check(client).await.unwrap().into_sui_object()
    }

    pub async fn check(self, client: &GatewayClient) -> Result<CheckerResultObject, anyhow::Error> {
        debug!(?self);

        let object_id = self.object_id;
        let object_info = client
            .get_object(object_id)
            .await
            .or_else(|err| bail!("Failed to get object info (id: {}), err: {err}", object_id))?;
        match object_info {
            GetObjectDataResponse::NotExists(_) => {
                panic!("Node can't find gas object {}", object_id)
            }
            GetObjectDataResponse::Deleted(_) => {
                if !self.is_deleted {
                    panic!("Gas object {} was deleted", object_id);
                }
                Ok(CheckerResultObject::new(None, None))
            }
            GetObjectDataResponse::Exists(object) => {
                if self.is_deleted {
                    panic!("Expect Gas object {} deleted, but it is not", object_id);
                }
                if let Some(owner) = self.owner {
                    assert_eq!(
                        object.owner, owner,
                        "Gas coin {} does not belong to {}, but {}",
                        object_id, owner, object.owner
                    );
                }
                if self.is_sui_coin == Some(true) {
                    let move_obj = object
                        .data
                        .try_as_move()
                        .unwrap_or_else(|| panic!("Object {} is not a move object", object_id));

                    let gas_coin = GasCoin::try_from(&move_obj.fields).unwrap_or_else(|err| {
                        panic!("Object {} is not a gas coin, {}", object_id, err)
                    });
                    return Ok(CheckerResultObject::new(Some(gas_coin), Some(object)));
                }
                Ok(CheckerResultObject::new(None, Some(object)))
            }
        }
    }
}

pub struct CheckerResultObject {
    gas_coin: Option<GasCoin>,
    sui_object: Option<SuiObject<SuiParsedMoveObject>>,
}

impl CheckerResultObject {
    pub fn new(
        gas_coin: Option<GasCoin>,
        sui_object: Option<SuiObject<SuiParsedMoveObject>>,
    ) -> Self {
        Self {
            gas_coin,
            sui_object,
        }
    }
    pub fn into_gas_coin(self) -> GasCoin {
        self.gas_coin.unwrap()
    }
    pub fn into_sui_object(self) -> SuiObject<SuiParsedMoveObject> {
        self.sui_object.unwrap()
    }
}

#[macro_export]
macro_rules! assert_eq_if_present {
    ($left:expr, $right:expr, $($arg:tt)+) => {
        match (&$left, &$right) {
            (Some(left_val), right_val) => {
                 if !(&left_val == right_val) {
                    panic!("{} does not match, left: {:?}, right: {:?}", $($arg)+, left_val, right_val);
                }
            }
            _ => ()
        }
    };
}

#[derive(Default, Debug)]
pub struct TransferObjectEventChecker {
    package_id: Option<ObjectID>,
    transaction_module: Option<String>,
    sender: Option<SuiAddress>,
    recipient: Option<Owner>,
    object_id: Option<ObjectID>,
    version: Option<SequenceNumber>,
    type_: Option<TransferType>,
}

impl TransferObjectEventChecker {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn package_id(mut self, package_id: ObjectID) -> Self {
        self.package_id = Some(package_id);
        self
    }
    pub fn transaction_module(mut self, transaction_module: String) -> Self {
        self.transaction_module = Some(transaction_module);
        self
    }
    pub fn sender(mut self, sender: SuiAddress) -> Self {
        self.sender = Some(sender);
        self
    }
    pub fn recipient(mut self, recipient: Owner) -> Self {
        self.recipient = Some(recipient);
        self
    }
    pub fn object_id(mut self, object_id: ObjectID) -> Self {
        self.object_id = Some(object_id);
        self
    }
    pub fn version(mut self, version: SequenceNumber) -> Self {
        self.version = Some(version);
        self
    }
    pub fn type_(mut self, type_: TransferType) -> Self {
        self.type_ = Some(type_);
        self
    }

    pub fn check(self, event: &SuiEvent) {
        if let SuiEvent::TransferObject {
            package_id,
            transaction_module,
            sender,
            recipient,
            object_id,
            version,
            type_,
        } = event
        {
            assert_eq_if_present!(self.package_id, package_id, "package_id");
            assert_eq_if_present!(
                self.transaction_module,
                transaction_module,
                "transaction_module"
            );
            assert_eq_if_present!(self.sender, sender, "sender");
            assert_eq_if_present!(self.recipient, recipient, "recipient");
            assert_eq_if_present!(self.object_id, object_id, "object_id");
            assert_eq_if_present!(self.version, version, "version");
            assert_eq_if_present!(self.type_, type_, "type_");
        } else {
            panic!("event {:?} is not TransferObject Event", event);
        }
    }
}
