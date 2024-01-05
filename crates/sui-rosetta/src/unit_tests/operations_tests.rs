// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use sui_types::base_types::{ObjectDigest, ObjectID, SequenceNumber, SuiAddress};
use sui_types::messages::TransactionData;

use crate::operations::Operations;
use crate::types::{ConstructionMetadata, TransactionMetadata};

#[tokio::test]
async fn test_operation_data_parsing() -> Result<(), anyhow::Error> {
    let gas = (
        ObjectID::random(),
        SequenceNumber::new(),
        ObjectDigest::random(),
    );

    let sender = SuiAddress::random_for_testing_only();

    let data = TransactionData::new_pay_sui(
        sender,
        vec![gas],
        vec![SuiAddress::random_for_testing_only()],
        vec![10000],
        gas,
        1000,
        /* gas price */ 1,
    );

    let ops: Operations = data.clone().try_into()?;
    let metadata = ConstructionMetadata {
        tx_metadata: TransactionMetadata::PaySui(vec![gas]),
        sender,
        gas,
        budget: 1000,
    };
    let parsed_data = ops.into_internal()?.into_data(metadata);
    assert_eq!(data, parsed_data);

    Ok(())
}
