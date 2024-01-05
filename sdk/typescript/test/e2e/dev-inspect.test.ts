// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeAll } from 'vitest';
import {
  getObjectId,
  getNewlyCreatedCoinRefsAfterSplit,
  RawSigner,
  UnserializedSignableTransaction,
} from '../../src';
import {
  DEFAULT_GAS_BUDGET,
  DEFAULT_RECIPIENT,
  publishPackage,
  setup,
  TestToolbox,
} from './utils/setup';

describe.skip('Test dev inspect', () => {
  let toolbox: TestToolbox;
  let packageId: string;

  beforeAll(async () => {
    toolbox = await setup();
    const packagePath = __dirname + '/./data/serializer';
    packageId = await publishPackage(packagePath);
  });

  it('Dev inspect transaction with Pay', async () => {
    const gasBudget = 1000;
    const coins =
      await toolbox.provider.selectCoinsWithBalanceGreaterThanOrEqual(
        toolbox.address(),
        BigInt(DEFAULT_GAS_BUDGET),
      );

    const splitTxn = await toolbox.signer.signAndExecuteTransaction({
      kind: 'splitCoin',
      data: {
        coinObjectId: coins[0].coinObjectId,
        splitAmounts: [2000, 2000, 2000],
        gasBudget: gasBudget,
        gasPayment: coins[1].coinObjectId,
      },
    });
    const splitCoins = getNewlyCreatedCoinRefsAfterSplit(splitTxn)!.map((c) =>
      getObjectId(c),
    );

    await validateDevInspectTransaction(
      toolbox.signer,
      {
        kind: 'pay',
        data: {
          inputCoins: splitCoins,
          recipients: [DEFAULT_RECIPIENT],
          amounts: ['4000'],
          gasBudget: gasBudget,
        },
      },
      'success',
    );
  });

  it('Move Call that returns struct', async () => {
    const coins = await toolbox.getGasObjectsOwnedByAddress();
    const moveCall = {
      packageObjectId: packageId,
      module: 'serializer_tests',
      function: 'return_struct',
      typeArguments: ['0x2::coin::Coin<0x2::sui::SUI>'],
      arguments: [coins[0].objectId],
      gasBudget: DEFAULT_GAS_BUDGET,
    };

    await validateDevInspectTransaction(
      toolbox.signer,
      {
        kind: 'moveCall',
        data: moveCall,
      },
      'success',
    );
  });

  it('Move Call that aborts', async () => {
    const moveCall = {
      packageObjectId: packageId,
      module: 'serializer_tests',
      function: 'test_abort',
      typeArguments: [],
      arguments: [],
      gasBudget: DEFAULT_GAS_BUDGET,
    };

    await validateDevInspectTransaction(
      toolbox.signer,
      {
        kind: 'moveCall',
        data: moveCall,
      },
      'failure',
    );
  });
});

async function validateDevInspectTransaction(
  signer: RawSigner,
  txn: UnserializedSignableTransaction,
  status: 'success' | 'failure',
) {
  const result = await signer.devInspectTransaction(txn);
  expect(result.effects.status.status).toEqual(status);
}
