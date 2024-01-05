// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeAll } from 'vitest';
import {
  Coin,
  getCreatedObjects,
  getExecutionStatusType,
  ObjectId,
  SUI_FRAMEWORK_ADDRESS,
} from '../../src';
import {
  DEFAULT_GAS_BUDGET,
  publishPackage,
  setup,
  TestToolbox,
} from './utils/setup';

describe.skip('Test Move call with a vector of objects as input (skipped due to move vector requirement)', () => {
  let toolbox: TestToolbox;
  let packageId: ObjectId;

  async function mintObject(val: number) {
    const txn = await toolbox.signer.signAndExecuteTransaction({
      kind: 'moveCall',
      data: {
        packageObjectId: packageId,
        module: 'entry_point_vector',
        function: 'mint',
        typeArguments: [],
        arguments: [val.toString()],
        gasBudget: DEFAULT_GAS_BUDGET,
      },
    });
    expect(getExecutionStatusType(txn)).toEqual('success');
    return getCreatedObjects(txn)![0].reference.objectId;
  }

  async function destroyObjects(objects: ObjectId[]) {
    const txn = await toolbox.signer.signAndExecuteTransaction({
      kind: 'moveCall',
      data: {
        packageObjectId: packageId,
        module: 'entry_point_vector',
        function: 'two_obj_vec_destroy',
        typeArguments: [],
        arguments: [objects],
        gasBudget: DEFAULT_GAS_BUDGET,
      },
    });
    expect(getExecutionStatusType(txn)).toEqual('success');
  }

  beforeAll(async () => {
    toolbox = await setup();
    const packagePath =
      __dirname +
      '/../../../../crates/sui-core/src/unit_tests/data/entry_point_vector';
    packageId = await publishPackage(packagePath);
  });

  it('Test object vector', async () => {
    await destroyObjects([await mintObject(7), await mintObject(42)]);
  });

  it('Test regular arg mixed with object vector arg', async () => {
    const coins = await toolbox.getGasObjectsOwnedByAddress();
    const coinIDs = coins.map((coin) => Coin.getID(coin));
    const txn = await toolbox.signer.signAndExecuteTransaction({
      kind: 'moveCall',
      data: {
        packageObjectId: SUI_FRAMEWORK_ADDRESS,
        module: 'pay',
        function: 'join_vec',
        typeArguments: ['0x2::sui::SUI'],
        arguments: [coinIDs[0], [coinIDs[1], coinIDs[2]]],
        gasBudget: DEFAULT_GAS_BUDGET,
      },
    });
    expect(getExecutionStatusType(txn)).toEqual('success');
  });
});
