// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// A synthetic fungible token backed by a basket of other tokens.
/// Here, we use a basket that is 1:1 SUI and MANAGED,
/// but this approach would work for a basket with arbitrary assets/ratios.
/// E.g., [SDR](https://www.imf.org/en/About/Factsheets/Sheets/2016/08/01/14/51/Special-Drawing-Right-SDR)
/// could be implemented this way.
module fungible_tokens::basket {
    use fungible_tokens::managed::MANAGED;
    use sui::coin::{Self, Coin, TreasuryCap};
    use sui::balance::{Self, Balance};
    use sui::id::VersionedID;
    use sui::sui::SUI;
    use sui::transfer;
    use sui::tx_context::{Self, TxContext};

    /// Name of the coin. By convention, this type has the same name as its parent module
    /// and has no fields. The full type of the coin defined by this module will be `COIN<BASKET>`.
    struct BASKET has drop { }

    /// Singleton shared object holding the reserve assets and the capability.
    struct Reserve has key {
        id: VersionedID,
        /// capability allowing the reserve to mint and burn BASKET
        treasury_cap: TreasuryCap<BASKET>,
        /// SUI coins held in the reserve
        sui: Balance<SUI>,
        /// MANAGED coins held in the reserve
        managed: Balance<MANAGED>,
    }

    /// Needed to deposit a 1:1 ratio of SUI and MANAGED for minting, but deposited a different ratio
    const EBadDepositRatio: u64 = 0;

    fun init(ctx: &mut TxContext) {
        // Get a treasury cap for the coin put it in the reserve
        let treasury_cap = coin::create_currency<BASKET>(BASKET{}, ctx);
        transfer::share_object(Reserve {
            id: tx_context::new_id(ctx),
            treasury_cap,
            sui: balance::zero<SUI>(),
            managed: balance::zero<MANAGED>(),
        })
    }

    /// === Writes ===

    /// Mint BASKET coins by accepting an equal number of SUI and MANAGED coins
    public fun mint(
        reserve: &mut Reserve, sui: Coin<SUI>, managed: Coin<MANAGED>, ctx: &mut TxContext
    ): Coin<BASKET> {
        let num_sui = coin::value(&sui);
        assert!(num_sui == coin::value(&managed), EBadDepositRatio);

        coin::deposit(&mut reserve.sui, sui);
        coin::deposit(&mut reserve.managed, managed);
        coin::mint(num_sui, &mut reserve.treasury_cap, ctx)
    }

    /// Burn BASKET coins and return the underlying reserve assets
    public fun burn(
        reserve: &mut Reserve, basket: Coin<BASKET>, ctx: &mut TxContext
    ): (Coin<SUI>, Coin<MANAGED>) {
        let num_basket = coin::value(&basket);
        coin::burn(basket, &mut reserve.treasury_cap);
        let sui = coin::withdraw(&mut reserve.sui, num_basket, ctx);
        let managed = coin::withdraw(&mut reserve.managed, num_basket, ctx);
        (sui, managed)
    }

    // === Reads ===

    /// Return the number of `MANAGED` coins in circulation
    public fun total_supply(reserve: &Reserve): u64 {
        coin::total_supply(&reserve.treasury_cap)
    }

    /// Return the number of SUI in the reserve
    public fun sui_supply(reserve: &Reserve): u64 {
        balance::value(&reserve.sui)
    }

    /// Return the number of MANAGED in the reserve
    public fun managed_supply(reserve: &Reserve): u64 {
        balance::value(&reserve.managed)
    }

    #[test_only]
    public fun init_for_testing(ctx: &mut TxContext) {
        init(ctx)
    }
}
