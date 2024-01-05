
<a name="0x2_validator"></a>

# Module `0x2::validator`



-  [Struct `ValidatorMetadata`](#0x2_validator_ValidatorMetadata)
-  [Struct `Validator`](#0x2_validator_Validator)
-  [Constants](#@Constants_0)
-  [Function `verify_proof_of_possession`](#0x2_validator_verify_proof_of_possession)
-  [Function `new_metadata`](#0x2_validator_new_metadata)
-  [Function `new`](#0x2_validator_new)
-  [Function `destroy`](#0x2_validator_destroy)
-  [Function `adjust_stake_and_gas_price`](#0x2_validator_adjust_stake_and_gas_price)
-  [Function `request_add_delegation`](#0x2_validator_request_add_delegation)
-  [Function `request_withdraw_delegation`](#0x2_validator_request_withdraw_delegation)
-  [Function `request_set_gas_price`](#0x2_validator_request_set_gas_price)
-  [Function `request_set_commission_rate`](#0x2_validator_request_set_commission_rate)
-  [Function `deposit_delegation_rewards`](#0x2_validator_deposit_delegation_rewards)
-  [Function `process_pending_delegations_and_withdraws`](#0x2_validator_process_pending_delegations_and_withdraws)
-  [Function `get_staking_pool_mut_ref`](#0x2_validator_get_staking_pool_mut_ref)
-  [Function `metadata`](#0x2_validator_metadata)
-  [Function `sui_address`](#0x2_validator_sui_address)
-  [Function `name`](#0x2_validator_name)
-  [Function `description`](#0x2_validator_description)
-  [Function `image_url`](#0x2_validator_image_url)
-  [Function `project_url`](#0x2_validator_project_url)
-  [Function `network_address`](#0x2_validator_network_address)
-  [Function `p2p_address`](#0x2_validator_p2p_address)
-  [Function `consensus_address`](#0x2_validator_consensus_address)
-  [Function `worker_address`](#0x2_validator_worker_address)
-  [Function `protocol_pubkey_bytes`](#0x2_validator_protocol_pubkey_bytes)
-  [Function `proof_of_possession`](#0x2_validator_proof_of_possession)
-  [Function `network_pubkey_bytes`](#0x2_validator_network_pubkey_bytes)
-  [Function `worker_pubkey_bytes`](#0x2_validator_worker_pubkey_bytes)
-  [Function `next_epoch_network_address`](#0x2_validator_next_epoch_network_address)
-  [Function `next_epoch_p2p_address`](#0x2_validator_next_epoch_p2p_address)
-  [Function `next_epoch_consensus_address`](#0x2_validator_next_epoch_consensus_address)
-  [Function `next_epoch_worker_address`](#0x2_validator_next_epoch_worker_address)
-  [Function `next_epoch_protocol_pubkey_bytes`](#0x2_validator_next_epoch_protocol_pubkey_bytes)
-  [Function `next_epoch_proof_of_possession`](#0x2_validator_next_epoch_proof_of_possession)
-  [Function `next_epoch_network_pubkey_bytes`](#0x2_validator_next_epoch_network_pubkey_bytes)
-  [Function `next_epoch_worker_pubkey_bytes`](#0x2_validator_next_epoch_worker_pubkey_bytes)
-  [Function `total_stake_amount`](#0x2_validator_total_stake_amount)
-  [Function `delegate_amount`](#0x2_validator_delegate_amount)
-  [Function `total_stake`](#0x2_validator_total_stake)
-  [Function `voting_power`](#0x2_validator_voting_power)
-  [Function `set_voting_power`](#0x2_validator_set_voting_power)
-  [Function `pending_stake_amount`](#0x2_validator_pending_stake_amount)
-  [Function `pending_stake_withdraw_amount`](#0x2_validator_pending_stake_withdraw_amount)
-  [Function `gas_price`](#0x2_validator_gas_price)
-  [Function `commission_rate`](#0x2_validator_commission_rate)
-  [Function `pool_token_exchange_rate_at_epoch`](#0x2_validator_pool_token_exchange_rate_at_epoch)
-  [Function `staking_pool_id`](#0x2_validator_staking_pool_id)
-  [Function `is_duplicate`](#0x2_validator_is_duplicate)
-  [Function `update_name`](#0x2_validator_update_name)
-  [Function `update_description`](#0x2_validator_update_description)
-  [Function `update_image_url`](#0x2_validator_update_image_url)
-  [Function `update_project_url`](#0x2_validator_update_project_url)
-  [Function `update_next_epoch_network_address`](#0x2_validator_update_next_epoch_network_address)
-  [Function `update_next_epoch_p2p_address`](#0x2_validator_update_next_epoch_p2p_address)
-  [Function `update_next_epoch_consensus_address`](#0x2_validator_update_next_epoch_consensus_address)
-  [Function `update_next_epoch_worker_address`](#0x2_validator_update_next_epoch_worker_address)
-  [Function `update_next_epoch_protocol_pubkey`](#0x2_validator_update_next_epoch_protocol_pubkey)
-  [Function `update_next_epoch_network_pubkey`](#0x2_validator_update_next_epoch_network_pubkey)
-  [Function `update_next_epoch_worker_pubkey`](#0x2_validator_update_next_epoch_worker_pubkey)
-  [Function `effectuate_staged_metadata`](#0x2_validator_effectuate_staged_metadata)
-  [Function `validate_metadata`](#0x2_validator_validate_metadata)
-  [Function `validate_metadata_bcs`](#0x2_validator_validate_metadata_bcs)


<pre><code><b>use</b> <a href="">0x1::ascii</a>;
<b>use</b> <a href="">0x1::bcs</a>;
<b>use</b> <a href="">0x1::option</a>;
<b>use</b> <a href="">0x1::string</a>;
<b>use</b> <a href="">0x1::vector</a>;
<b>use</b> <a href="balance.md#0x2_balance">0x2::balance</a>;
<b>use</b> <a href="bcs.md#0x2_bcs">0x2::bcs</a>;
<b>use</b> <a href="bls12381.md#0x2_bls12381">0x2::bls12381</a>;
<b>use</b> <a href="epoch_time_lock.md#0x2_epoch_time_lock">0x2::epoch_time_lock</a>;
<b>use</b> <a href="object.md#0x2_object">0x2::object</a>;
<b>use</b> <a href="staking_pool.md#0x2_staking_pool">0x2::staking_pool</a>;
<b>use</b> <a href="sui.md#0x2_sui">0x2::sui</a>;
<b>use</b> <a href="tx_context.md#0x2_tx_context">0x2::tx_context</a>;
<b>use</b> <a href="url.md#0x2_url">0x2::url</a>;
</code></pre>



<a name="0x2_validator_ValidatorMetadata"></a>

## Struct `ValidatorMetadata`



<pre><code><b>struct</b> <a href="validator.md#0x2_validator_ValidatorMetadata">ValidatorMetadata</a> <b>has</b> <b>copy</b>, drop, store
</code></pre>



<details>
<summary>Fields</summary>


<dl>
<dt>
<code>sui_address: <b>address</b></code>
</dt>
<dd>
 The Sui Address of the validator. This is the sender that created the Validator object,
 and also the address to send validator/coins to during withdraws.
</dd>
<dt>
<code>protocol_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;</code>
</dt>
<dd>
 The public key bytes corresponding to the private key that the validator
 holds to sign transactions. For now, this is the same as AuthorityName.
</dd>
<dt>
<code>network_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;</code>
</dt>
<dd>
 The public key bytes corresponding to the private key that the validator
 uses to establish TLS connections
</dd>
<dt>
<code>worker_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;</code>
</dt>
<dd>
 The public key bytes correstponding to the Narwhal Worker
</dd>
<dt>
<code>proof_of_possession: <a href="">vector</a>&lt;u8&gt;</code>
</dt>
<dd>
 This is a proof that the validator has ownership of the private key
</dd>
<dt>
<code>name: <a href="_String">string::String</a></code>
</dt>
<dd>
 A unique human-readable name of this validator.
</dd>
<dt>
<code>description: <a href="_String">string::String</a></code>
</dt>
<dd>

</dd>
<dt>
<code>image_url: <a href="url.md#0x2_url_Url">url::Url</a></code>
</dt>
<dd>

</dd>
<dt>
<code>project_url: <a href="url.md#0x2_url_Url">url::Url</a></code>
</dt>
<dd>

</dd>
<dt>
<code>net_address: <a href="">vector</a>&lt;u8&gt;</code>
</dt>
<dd>
 The network address of the validator (could also contain extra info such as port, DNS and etc.).
</dd>
<dt>
<code>p2p_address: <a href="">vector</a>&lt;u8&gt;</code>
</dt>
<dd>
 The address of the validator used for p2p activities such as state sync (could also contain extra info such as port, DNS and etc.).
</dd>
<dt>
<code>consensus_address: <a href="">vector</a>&lt;u8&gt;</code>
</dt>
<dd>
 The address of the narwhal primary
</dd>
<dt>
<code>worker_address: <a href="">vector</a>&lt;u8&gt;</code>
</dt>
<dd>
 The address of the narwhal worker
</dd>
<dt>
<code>next_epoch_protocol_pubkey_bytes: <a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;</code>
</dt>
<dd>
 "next_epoch" metadata only takes effects in the next epoch.
 If none, current value will stay unchanged.
</dd>
<dt>
<code>next_epoch_proof_of_possession: <a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;</code>
</dt>
<dd>

</dd>
<dt>
<code>next_epoch_network_pubkey_bytes: <a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;</code>
</dt>
<dd>

</dd>
<dt>
<code>next_epoch_worker_pubkey_bytes: <a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;</code>
</dt>
<dd>

</dd>
<dt>
<code>next_epoch_net_address: <a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;</code>
</dt>
<dd>

</dd>
<dt>
<code>next_epoch_p2p_address: <a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;</code>
</dt>
<dd>

</dd>
<dt>
<code>next_epoch_consensus_address: <a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;</code>
</dt>
<dd>

</dd>
<dt>
<code>next_epoch_worker_address: <a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;</code>
</dt>
<dd>

</dd>
</dl>


</details>

<a name="0x2_validator_Validator"></a>

## Struct `Validator`



<pre><code><b>struct</b> <a href="validator.md#0x2_validator_Validator">Validator</a> <b>has</b> store
</code></pre>



<details>
<summary>Fields</summary>


<dl>
<dt>
<code>metadata: <a href="validator.md#0x2_validator_ValidatorMetadata">validator::ValidatorMetadata</a></code>
</dt>
<dd>
 Summary of the validator.
</dd>
<dt>
<code><a href="voting_power.md#0x2_voting_power">voting_power</a>: u64</code>
</dt>
<dd>
 The voting power of this validator, which might be different from its
 stake amount.
</dd>
<dt>
<code>gas_price: u64</code>
</dt>
<dd>
 Gas price quote, updated only at end of epoch.
</dd>
<dt>
<code><a href="staking_pool.md#0x2_staking_pool">staking_pool</a>: <a href="staking_pool.md#0x2_staking_pool_StakingPool">staking_pool::StakingPool</a></code>
</dt>
<dd>
 Staking pool for the stakes delegated to this validator.
</dd>
<dt>
<code>commission_rate: u64</code>
</dt>
<dd>
 Commission rate of the validator, in basis point.
</dd>
<dt>
<code>next_epoch_stake: u64</code>
</dt>
<dd>
 Total amount of stake that would be active in the next epoch.
</dd>
<dt>
<code>next_epoch_gas_price: u64</code>
</dt>
<dd>
 This validator's gas price quote for the next epoch.
</dd>
<dt>
<code>next_epoch_commission_rate: u64</code>
</dt>
<dd>
 The commission rate of the validator starting the next epoch, in basis point.
</dd>
</dl>


</details>

<a name="@Constants_0"></a>

## Constants


<a name="0x2_validator_EInvalidProofOfPossession"></a>



<pre><code><b>const</b> <a href="validator.md#0x2_validator_EInvalidProofOfPossession">EInvalidProofOfPossession</a>: u64 = 0;
</code></pre>



<a name="0x2_validator_EMetadataInvalidConsensusAddr"></a>

Invalid consensus_address field in ValidatorMetadata


<pre><code><b>const</b> <a href="validator.md#0x2_validator_EMetadataInvalidConsensusAddr">EMetadataInvalidConsensusAddr</a>: u64 = 6;
</code></pre>



<a name="0x2_validator_EMetadataInvalidNetAddr"></a>

Invalid net_address field in ValidatorMetadata


<pre><code><b>const</b> <a href="validator.md#0x2_validator_EMetadataInvalidNetAddr">EMetadataInvalidNetAddr</a>: u64 = 4;
</code></pre>



<a name="0x2_validator_EMetadataInvalidNetPubkey"></a>

Invalid network_pubkey_bytes field in ValidatorMetadata


<pre><code><b>const</b> <a href="validator.md#0x2_validator_EMetadataInvalidNetPubkey">EMetadataInvalidNetPubkey</a>: u64 = 2;
</code></pre>



<a name="0x2_validator_EMetadataInvalidP2pAddr"></a>

Invalid p2p_address field in ValidatorMetadata


<pre><code><b>const</b> <a href="validator.md#0x2_validator_EMetadataInvalidP2pAddr">EMetadataInvalidP2pAddr</a>: u64 = 5;
</code></pre>



<a name="0x2_validator_EMetadataInvalidPubKey"></a>

Invalid pubkey_bytes field in ValidatorMetadata


<pre><code><b>const</b> <a href="validator.md#0x2_validator_EMetadataInvalidPubKey">EMetadataInvalidPubKey</a>: u64 = 1;
</code></pre>



<a name="0x2_validator_EMetadataInvalidWorkerAddr"></a>

Invalidworker_address field in ValidatorMetadata


<pre><code><b>const</b> <a href="validator.md#0x2_validator_EMetadataInvalidWorkerAddr">EMetadataInvalidWorkerAddr</a>: u64 = 7;
</code></pre>



<a name="0x2_validator_EMetadataInvalidWorkerPubKey"></a>

Invalid worker_pubkey_bytes field in ValidatorMetadata


<pre><code><b>const</b> <a href="validator.md#0x2_validator_EMetadataInvalidWorkerPubKey">EMetadataInvalidWorkerPubKey</a>: u64 = 3;
</code></pre>



<a name="0x2_validator_PROOF_OF_POSSESSION_DOMAIN"></a>



<pre><code><b>const</b> <a href="validator.md#0x2_validator_PROOF_OF_POSSESSION_DOMAIN">PROOF_OF_POSSESSION_DOMAIN</a>: <a href="">vector</a>&lt;u8&gt; = [107, 111, 115, 107];
</code></pre>



<a name="0x2_validator_verify_proof_of_possession"></a>

## Function `verify_proof_of_possession`



<pre><code><b>fun</b> <a href="validator.md#0x2_validator_verify_proof_of_possession">verify_proof_of_possession</a>(proof_of_possession: <a href="">vector</a>&lt;u8&gt;, sui_address: <b>address</b>, protocol_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>fun</b> <a href="validator.md#0x2_validator_verify_proof_of_possession">verify_proof_of_possession</a>(
    proof_of_possession: <a href="">vector</a>&lt;u8&gt;,
    sui_address: <b>address</b>,
    protocol_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;
) {
    // The proof of possession is the signature over ValidatorPK || AccountAddress.
    // This proves that the account <b>address</b> is owned by the holder of ValidatorPK, and <b>ensures</b>
    // that PK <b>exists</b>.
    <b>let</b> signed_bytes = protocol_pubkey_bytes;
    <b>let</b> address_bytes = to_bytes(&sui_address);
    <a href="_append">vector::append</a>(&<b>mut</b> signed_bytes, address_bytes);
    <b>assert</b>!(
        bls12381_min_sig_verify_with_domain(&proof_of_possession, &protocol_pubkey_bytes, signed_bytes, <a href="validator.md#0x2_validator_PROOF_OF_POSSESSION_DOMAIN">PROOF_OF_POSSESSION_DOMAIN</a>) == <b>true</b>,
        <a href="validator.md#0x2_validator_EInvalidProofOfPossession">EInvalidProofOfPossession</a>
    );
}
</code></pre>



</details>

<a name="0x2_validator_new_metadata"></a>

## Function `new_metadata`



<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_new_metadata">new_metadata</a>(sui_address: <b>address</b>, protocol_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;, network_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;, worker_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;, proof_of_possession: <a href="">vector</a>&lt;u8&gt;, name: <a href="_String">string::String</a>, description: <a href="_String">string::String</a>, image_url: <a href="url.md#0x2_url_Url">url::Url</a>, project_url: <a href="url.md#0x2_url_Url">url::Url</a>, net_address: <a href="">vector</a>&lt;u8&gt;, p2p_address: <a href="">vector</a>&lt;u8&gt;, consensus_address: <a href="">vector</a>&lt;u8&gt;, worker_address: <a href="">vector</a>&lt;u8&gt;): <a href="validator.md#0x2_validator_ValidatorMetadata">validator::ValidatorMetadata</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_new_metadata">new_metadata</a>(
    sui_address: <b>address</b>,
    protocol_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;,
    network_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;,
    worker_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;,
    proof_of_possession: <a href="">vector</a>&lt;u8&gt;,
    name: String,
    description: String,
    image_url: Url,
    project_url: Url,
    net_address: <a href="">vector</a>&lt;u8&gt;,
    p2p_address: <a href="">vector</a>&lt;u8&gt;,
    consensus_address: <a href="">vector</a>&lt;u8&gt;,
    worker_address: <a href="">vector</a>&lt;u8&gt;,
): <a href="validator.md#0x2_validator_ValidatorMetadata">ValidatorMetadata</a> {
    <b>let</b> metadata = <a href="validator.md#0x2_validator_ValidatorMetadata">ValidatorMetadata</a> {
        sui_address,
        protocol_pubkey_bytes,
        network_pubkey_bytes,
        worker_pubkey_bytes,
        proof_of_possession,
        name,
        description,
        image_url,
        project_url,
        net_address,
        p2p_address,
        consensus_address,
        worker_address,
        next_epoch_protocol_pubkey_bytes: <a href="_none">option::none</a>(),
        next_epoch_network_pubkey_bytes: <a href="_none">option::none</a>(),
        next_epoch_worker_pubkey_bytes: <a href="_none">option::none</a>(),
        next_epoch_proof_of_possession: <a href="_none">option::none</a>(),
        next_epoch_net_address: <a href="_none">option::none</a>(),
        next_epoch_p2p_address: <a href="_none">option::none</a>(),
        next_epoch_consensus_address: <a href="_none">option::none</a>(),
        next_epoch_worker_address: <a href="_none">option::none</a>(),
    };
    metadata
}
</code></pre>



</details>

<a name="0x2_validator_new"></a>

## Function `new`



<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_new">new</a>(sui_address: <b>address</b>, protocol_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;, network_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;, worker_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;, proof_of_possession: <a href="">vector</a>&lt;u8&gt;, name: <a href="">vector</a>&lt;u8&gt;, description: <a href="">vector</a>&lt;u8&gt;, image_url: <a href="">vector</a>&lt;u8&gt;, project_url: <a href="">vector</a>&lt;u8&gt;, net_address: <a href="">vector</a>&lt;u8&gt;, p2p_address: <a href="">vector</a>&lt;u8&gt;, consensus_address: <a href="">vector</a>&lt;u8&gt;, worker_address: <a href="">vector</a>&lt;u8&gt;, stake: <a href="balance.md#0x2_balance_Balance">balance::Balance</a>&lt;<a href="sui.md#0x2_sui_SUI">sui::SUI</a>&gt;, coin_locked_until_epoch: <a href="_Option">option::Option</a>&lt;<a href="epoch_time_lock.md#0x2_epoch_time_lock_EpochTimeLock">epoch_time_lock::EpochTimeLock</a>&gt;, gas_price: u64, commission_rate: u64, starting_epoch: u64, ctx: &<b>mut</b> <a href="tx_context.md#0x2_tx_context_TxContext">tx_context::TxContext</a>): <a href="validator.md#0x2_validator_Validator">validator::Validator</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_new">new</a>(
    sui_address: <b>address</b>,
    protocol_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;,
    network_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;,
    worker_pubkey_bytes: <a href="">vector</a>&lt;u8&gt;,
    proof_of_possession: <a href="">vector</a>&lt;u8&gt;,
    name: <a href="">vector</a>&lt;u8&gt;,
    description: <a href="">vector</a>&lt;u8&gt;,
    image_url: <a href="">vector</a>&lt;u8&gt;,
    project_url: <a href="">vector</a>&lt;u8&gt;,
    net_address: <a href="">vector</a>&lt;u8&gt;,
    p2p_address: <a href="">vector</a>&lt;u8&gt;,
    consensus_address: <a href="">vector</a>&lt;u8&gt;,
    worker_address: <a href="">vector</a>&lt;u8&gt;,
    stake: Balance&lt;SUI&gt;,
    coin_locked_until_epoch: Option&lt;EpochTimeLock&gt;,
    gas_price: u64,
    commission_rate: u64,
    starting_epoch: u64,
    ctx: &<b>mut</b> TxContext
): <a href="validator.md#0x2_validator_Validator">Validator</a> {
    <b>assert</b>!(
        // TODO: These constants are arbitrary, will adjust once we know more.
        <a href="_length">vector::length</a>(&net_address) &lt;= 128
            && <a href="_length">vector::length</a>(&p2p_address) &lt;= 128
            && <a href="_length">vector::length</a>(&name) &lt;= 128
            && <a href="_length">vector::length</a>(&description) &lt;= 150
            && <a href="_length">vector::length</a>(&protocol_pubkey_bytes) &lt;= 128,
        0
    );
    <a href="validator.md#0x2_validator_verify_proof_of_possession">verify_proof_of_possession</a>(
        proof_of_possession,
        sui_address,
        protocol_pubkey_bytes
    );
    <b>let</b> stake_amount = <a href="balance.md#0x2_balance_value">balance::value</a>(&stake);

    <b>let</b> metadata = <a href="validator.md#0x2_validator_new_metadata">new_metadata</a>(
        sui_address,
        protocol_pubkey_bytes,
        network_pubkey_bytes,
        worker_pubkey_bytes,
        proof_of_possession,
        <a href="_from_ascii">string::from_ascii</a>(<a href="_string">ascii::string</a>(name)),
        <a href="_from_ascii">string::from_ascii</a>(<a href="_string">ascii::string</a>(description)),
        <a href="url.md#0x2_url_new_unsafe_from_bytes">url::new_unsafe_from_bytes</a>(image_url),
        <a href="url.md#0x2_url_new_unsafe_from_bytes">url::new_unsafe_from_bytes</a>(project_url),
        net_address,
        p2p_address,
        consensus_address,
        worker_address,
    );

    <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(&metadata);
    <b>let</b> <a href="staking_pool.md#0x2_staking_pool">staking_pool</a> = <a href="staking_pool.md#0x2_staking_pool_new">staking_pool::new</a>(starting_epoch, ctx);
    // Add the <a href="validator.md#0x2_validator">validator</a>'s starting stake <b>to</b> the staking pool.
    <a href="staking_pool.md#0x2_staking_pool_request_add_delegation">staking_pool::request_add_delegation</a>(&<b>mut</b> <a href="staking_pool.md#0x2_staking_pool">staking_pool</a>, stake, coin_locked_until_epoch, sui_address, sui_address, starting_epoch, ctx);
    // We immediately process this delegation <b>as</b> they are at <a href="validator.md#0x2_validator">validator</a> setup time and this is the <a href="validator.md#0x2_validator">validator</a> staking <b>with</b> itself.
    <a href="staking_pool.md#0x2_staking_pool_process_pending_delegation">staking_pool::process_pending_delegation</a>(&<b>mut</b> <a href="staking_pool.md#0x2_staking_pool">staking_pool</a>);
    <a href="validator.md#0x2_validator_Validator">Validator</a> {
        metadata,
        // Initialize the voting power <b>to</b> be the same <b>as</b> the stake amount.
        // At the epoch change <b>where</b> this <a href="validator.md#0x2_validator">validator</a> is actually added <b>to</b> the
        // active <a href="validator.md#0x2_validator">validator</a> set, the voting power will be updated accordingly.
        <a href="voting_power.md#0x2_voting_power">voting_power</a>: stake_amount,
        gas_price,
        <a href="staking_pool.md#0x2_staking_pool">staking_pool</a>,
        commission_rate,
        next_epoch_stake: stake_amount,
        next_epoch_gas_price: gas_price,
        next_epoch_commission_rate: commission_rate,
    }
}
</code></pre>



</details>

<a name="0x2_validator_destroy"></a>

## Function `destroy`



<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_destroy">destroy</a>(self: <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, ctx: &<b>mut</b> <a href="tx_context.md#0x2_tx_context_TxContext">tx_context::TxContext</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_destroy">destroy</a>(self: <a href="validator.md#0x2_validator_Validator">Validator</a>, ctx: &<b>mut</b> TxContext) {
    <b>let</b> <a href="validator.md#0x2_validator_Validator">Validator</a> {
        metadata: _,
        <a href="voting_power.md#0x2_voting_power">voting_power</a>: _,
        gas_price: _,
        <a href="staking_pool.md#0x2_staking_pool">staking_pool</a>,
        commission_rate: _,
        next_epoch_stake: _,
        next_epoch_gas_price: _,
        next_epoch_commission_rate: _,
    } = self;
    <a href="staking_pool.md#0x2_staking_pool_deactivate_staking_pool">staking_pool::deactivate_staking_pool</a>(<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>, ctx);
}
</code></pre>



</details>

<a name="0x2_validator_adjust_stake_and_gas_price"></a>

## Function `adjust_stake_and_gas_price`

Process pending stake and pending withdraws, and update the gas price.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_adjust_stake_and_gas_price">adjust_stake_and_gas_price</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_adjust_stake_and_gas_price">adjust_stake_and_gas_price</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>) {
    self.gas_price = self.next_epoch_gas_price;
    self.commission_rate = self.next_epoch_commission_rate;
}
</code></pre>



</details>

<a name="0x2_validator_request_add_delegation"></a>

## Function `request_add_delegation`

Request to add delegation to the validator's staking pool, processed at the end of the epoch.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_request_add_delegation">request_add_delegation</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, delegated_stake: <a href="balance.md#0x2_balance_Balance">balance::Balance</a>&lt;<a href="sui.md#0x2_sui_SUI">sui::SUI</a>&gt;, locking_period: <a href="_Option">option::Option</a>&lt;<a href="epoch_time_lock.md#0x2_epoch_time_lock_EpochTimeLock">epoch_time_lock::EpochTimeLock</a>&gt;, delegator: <b>address</b>, ctx: &<b>mut</b> <a href="tx_context.md#0x2_tx_context_TxContext">tx_context::TxContext</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_request_add_delegation">request_add_delegation</a>(
    self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>,
    delegated_stake: Balance&lt;SUI&gt;,
    locking_period: Option&lt;EpochTimeLock&gt;,
    delegator: <b>address</b>,
    ctx: &<b>mut</b> TxContext,
) {
    <b>let</b> delegate_amount = <a href="balance.md#0x2_balance_value">balance::value</a>(&delegated_stake);
    <b>assert</b>!(delegate_amount &gt; 0, 0);
    <b>let</b> delegation_epoch = <a href="tx_context.md#0x2_tx_context_epoch">tx_context::epoch</a>(ctx) + 1;
    <a href="staking_pool.md#0x2_staking_pool_request_add_delegation">staking_pool::request_add_delegation</a>(
        &<b>mut</b> self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>, delegated_stake, locking_period, self.metadata.sui_address, delegator, delegation_epoch, ctx
    );
    self.next_epoch_stake = self.next_epoch_stake + delegate_amount;
}
</code></pre>



</details>

<a name="0x2_validator_request_withdraw_delegation"></a>

## Function `request_withdraw_delegation`

Request to withdraw delegation from the validator's staking pool, processed at the end of the epoch.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_request_withdraw_delegation">request_withdraw_delegation</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, staked_sui: <a href="staking_pool.md#0x2_staking_pool_StakedSui">staking_pool::StakedSui</a>, ctx: &<b>mut</b> <a href="tx_context.md#0x2_tx_context_TxContext">tx_context::TxContext</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_request_withdraw_delegation">request_withdraw_delegation</a>(
    self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>,
    staked_sui: StakedSui,
    ctx: &<b>mut</b> TxContext,
) {
    <b>let</b> withdraw_amount = <a href="staking_pool.md#0x2_staking_pool_request_withdraw_delegation">staking_pool::request_withdraw_delegation</a>(
            &<b>mut</b> self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>, staked_sui, ctx);
    self.next_epoch_stake = self.next_epoch_stake - withdraw_amount;
}
</code></pre>



</details>

<a name="0x2_validator_request_set_gas_price"></a>

## Function `request_set_gas_price`

Request to set new gas price for the next epoch.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_request_set_gas_price">request_set_gas_price</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, new_price: u64)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_request_set_gas_price">request_set_gas_price</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, new_price: u64) {
    self.next_epoch_gas_price = new_price;
}
</code></pre>



</details>

<a name="0x2_validator_request_set_commission_rate"></a>

## Function `request_set_commission_rate`



<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_request_set_commission_rate">request_set_commission_rate</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, new_commission_rate: u64)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_request_set_commission_rate">request_set_commission_rate</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, new_commission_rate: u64) {
    self.next_epoch_commission_rate = new_commission_rate;
}
</code></pre>



</details>

<a name="0x2_validator_deposit_delegation_rewards"></a>

## Function `deposit_delegation_rewards`

Deposit delegations rewards into the validator's staking pool, called at the end of the epoch.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_deposit_delegation_rewards">deposit_delegation_rewards</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, reward: <a href="balance.md#0x2_balance_Balance">balance::Balance</a>&lt;<a href="sui.md#0x2_sui_SUI">sui::SUI</a>&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_deposit_delegation_rewards">deposit_delegation_rewards</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, reward: Balance&lt;SUI&gt;) {
    self.next_epoch_stake = self.next_epoch_stake + <a href="balance.md#0x2_balance_value">balance::value</a>(&reward);
    <a href="staking_pool.md#0x2_staking_pool_deposit_rewards">staking_pool::deposit_rewards</a>(&<b>mut</b> self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>, reward);
}
</code></pre>



</details>

<a name="0x2_validator_process_pending_delegations_and_withdraws"></a>

## Function `process_pending_delegations_and_withdraws`

Process pending delegations and withdraws, called at the end of the epoch.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_process_pending_delegations_and_withdraws">process_pending_delegations_and_withdraws</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, ctx: &<b>mut</b> <a href="tx_context.md#0x2_tx_context_TxContext">tx_context::TxContext</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_process_pending_delegations_and_withdraws">process_pending_delegations_and_withdraws</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, ctx: &<b>mut</b> TxContext) {
    <a href="staking_pool.md#0x2_staking_pool_process_pending_delegations_and_withdraws">staking_pool::process_pending_delegations_and_withdraws</a>(&<b>mut</b> self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>, ctx);
    <b>assert</b>!(<a href="validator.md#0x2_validator_delegate_amount">delegate_amount</a>(self) == self.next_epoch_stake, 0);
}
</code></pre>



</details>

<a name="0x2_validator_get_staking_pool_mut_ref"></a>

## Function `get_staking_pool_mut_ref`

Called by <code><a href="validator_set.md#0x2_validator_set">validator_set</a></code> for handling delegation switches.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_get_staking_pool_mut_ref">get_staking_pool_mut_ref</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<b>mut</b> <a href="staking_pool.md#0x2_staking_pool_StakingPool">staking_pool::StakingPool</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_get_staking_pool_mut_ref">get_staking_pool_mut_ref</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>) : &<b>mut</b> StakingPool {
    &<b>mut</b> self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>
}
</code></pre>



</details>

<a name="0x2_validator_metadata"></a>

## Function `metadata`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_metadata">metadata</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="validator.md#0x2_validator_ValidatorMetadata">validator::ValidatorMetadata</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_metadata">metadata</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="validator.md#0x2_validator_ValidatorMetadata">ValidatorMetadata</a> {
    &self.metadata
}
</code></pre>



</details>

<a name="0x2_validator_sui_address"></a>

## Function `sui_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_sui_address">sui_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): <b>address</b>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_sui_address">sui_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): <b>address</b> {
    self.metadata.sui_address
}
</code></pre>



</details>

<a name="0x2_validator_name"></a>

## Function `name`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_name">name</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_String">string::String</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_name">name</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &String {
    &self.metadata.name
}
</code></pre>



</details>

<a name="0x2_validator_description"></a>

## Function `description`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_description">description</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_String">string::String</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_description">description</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &String {
    &self.metadata.description
}
</code></pre>



</details>

<a name="0x2_validator_image_url"></a>

## Function `image_url`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_image_url">image_url</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="url.md#0x2_url_Url">url::Url</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_image_url">image_url</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Url {
    &self.metadata.image_url
}
</code></pre>



</details>

<a name="0x2_validator_project_url"></a>

## Function `project_url`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_project_url">project_url</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="url.md#0x2_url_Url">url::Url</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_project_url">project_url</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Url {
    &self.metadata.project_url
}
</code></pre>



</details>

<a name="0x2_validator_network_address"></a>

## Function `network_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_network_address">network_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_network_address">network_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="">vector</a>&lt;u8&gt; {
    &self.metadata.net_address
}
</code></pre>



</details>

<a name="0x2_validator_p2p_address"></a>

## Function `p2p_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_p2p_address">p2p_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_p2p_address">p2p_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="">vector</a>&lt;u8&gt; {
    &self.metadata.p2p_address
}
</code></pre>



</details>

<a name="0x2_validator_consensus_address"></a>

## Function `consensus_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_consensus_address">consensus_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_consensus_address">consensus_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="">vector</a>&lt;u8&gt; {
    &self.metadata.consensus_address
}
</code></pre>



</details>

<a name="0x2_validator_worker_address"></a>

## Function `worker_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_worker_address">worker_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_worker_address">worker_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="">vector</a>&lt;u8&gt; {
    &self.metadata.worker_address
}
</code></pre>



</details>

<a name="0x2_validator_protocol_pubkey_bytes"></a>

## Function `protocol_pubkey_bytes`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_protocol_pubkey_bytes">protocol_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_protocol_pubkey_bytes">protocol_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="">vector</a>&lt;u8&gt; {
    &self.metadata.protocol_pubkey_bytes
}
</code></pre>



</details>

<a name="0x2_validator_proof_of_possession"></a>

## Function `proof_of_possession`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_proof_of_possession">proof_of_possession</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_proof_of_possession">proof_of_possession</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="">vector</a>&lt;u8&gt; {
    &self.metadata.proof_of_possession
}
</code></pre>



</details>

<a name="0x2_validator_network_pubkey_bytes"></a>

## Function `network_pubkey_bytes`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_network_pubkey_bytes">network_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_network_pubkey_bytes">network_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="">vector</a>&lt;u8&gt; {
    &self.metadata.network_pubkey_bytes
}
</code></pre>



</details>

<a name="0x2_validator_worker_pubkey_bytes"></a>

## Function `worker_pubkey_bytes`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_worker_pubkey_bytes">worker_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="">vector</a>&lt;u8&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_worker_pubkey_bytes">worker_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &<a href="">vector</a>&lt;u8&gt; {
    &self.metadata.worker_pubkey_bytes
}
</code></pre>



</details>

<a name="0x2_validator_next_epoch_network_address"></a>

## Function `next_epoch_network_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_network_address">next_epoch_network_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_network_address">next_epoch_network_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Option&lt;<a href="">vector</a>&lt;u8&gt;&gt; {
    &self.metadata.next_epoch_net_address
}
</code></pre>



</details>

<a name="0x2_validator_next_epoch_p2p_address"></a>

## Function `next_epoch_p2p_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_p2p_address">next_epoch_p2p_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_p2p_address">next_epoch_p2p_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Option&lt;<a href="">vector</a>&lt;u8&gt;&gt; {
    &self.metadata.next_epoch_p2p_address
}
</code></pre>



</details>

<a name="0x2_validator_next_epoch_consensus_address"></a>

## Function `next_epoch_consensus_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_consensus_address">next_epoch_consensus_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_consensus_address">next_epoch_consensus_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Option&lt;<a href="">vector</a>&lt;u8&gt;&gt; {
    &self.metadata.next_epoch_consensus_address
}
</code></pre>



</details>

<a name="0x2_validator_next_epoch_worker_address"></a>

## Function `next_epoch_worker_address`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_worker_address">next_epoch_worker_address</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_worker_address">next_epoch_worker_address</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Option&lt;<a href="">vector</a>&lt;u8&gt;&gt; {
    &self.metadata.next_epoch_worker_address
}
</code></pre>



</details>

<a name="0x2_validator_next_epoch_protocol_pubkey_bytes"></a>

## Function `next_epoch_protocol_pubkey_bytes`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_protocol_pubkey_bytes">next_epoch_protocol_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_protocol_pubkey_bytes">next_epoch_protocol_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Option&lt;<a href="">vector</a>&lt;u8&gt;&gt; {
    &self.metadata.next_epoch_protocol_pubkey_bytes
}
</code></pre>



</details>

<a name="0x2_validator_next_epoch_proof_of_possession"></a>

## Function `next_epoch_proof_of_possession`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_proof_of_possession">next_epoch_proof_of_possession</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_proof_of_possession">next_epoch_proof_of_possession</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Option&lt;<a href="">vector</a>&lt;u8&gt;&gt; {
    &self.metadata.next_epoch_proof_of_possession
}
</code></pre>



</details>

<a name="0x2_validator_next_epoch_network_pubkey_bytes"></a>

## Function `next_epoch_network_pubkey_bytes`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_network_pubkey_bytes">next_epoch_network_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_network_pubkey_bytes">next_epoch_network_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Option&lt;<a href="">vector</a>&lt;u8&gt;&gt; {
    &self.metadata.next_epoch_network_pubkey_bytes
}
</code></pre>



</details>

<a name="0x2_validator_next_epoch_worker_pubkey_bytes"></a>

## Function `next_epoch_worker_pubkey_bytes`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_worker_pubkey_bytes">next_epoch_worker_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): &<a href="_Option">option::Option</a>&lt;<a href="">vector</a>&lt;u8&gt;&gt;
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_next_epoch_worker_pubkey_bytes">next_epoch_worker_pubkey_bytes</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): &Option&lt;<a href="">vector</a>&lt;u8&gt;&gt; {
    &self.metadata.next_epoch_worker_pubkey_bytes
}
</code></pre>



</details>

<a name="0x2_validator_total_stake_amount"></a>

## Function `total_stake_amount`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_total_stake_amount">total_stake_amount</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): u64
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_total_stake_amount">total_stake_amount</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): u64 {
    <b>spec</b> {
        // TODO: this should be provable rather than assumed
        <b>assume</b> self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>.sui_balance &lt;= MAX_U64;
    };
    <a href="staking_pool.md#0x2_staking_pool_sui_balance">staking_pool::sui_balance</a>(&self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>)
}
</code></pre>



</details>

<details>
<summary>Specification</summary>



<pre><code><b>aborts_if</b> <b>false</b>;
</code></pre>



</details>

<a name="0x2_validator_delegate_amount"></a>

## Function `delegate_amount`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_delegate_amount">delegate_amount</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): u64
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_delegate_amount">delegate_amount</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): u64 {
    <a href="staking_pool.md#0x2_staking_pool_sui_balance">staking_pool::sui_balance</a>(&self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>)
}
</code></pre>



</details>

<a name="0x2_validator_total_stake"></a>

## Function `total_stake`

Return the total amount staked with this validator


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_total_stake">total_stake</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): u64
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_total_stake">total_stake</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): u64 {
    <a href="validator.md#0x2_validator_delegate_amount">delegate_amount</a>(self)
}
</code></pre>



</details>

<a name="0x2_validator_voting_power"></a>

## Function `voting_power`

Return the voting power of this validator.


<pre><code><b>public</b> <b>fun</b> <a href="voting_power.md#0x2_voting_power">voting_power</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): u64
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="voting_power.md#0x2_voting_power">voting_power</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): u64 {
    self.<a href="voting_power.md#0x2_voting_power">voting_power</a>
}
</code></pre>



</details>

<a name="0x2_validator_set_voting_power"></a>

## Function `set_voting_power`

Set the voting power of this validator, called only from validator_set.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_set_voting_power">set_voting_power</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, new_voting_power: u64)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_set_voting_power">set_voting_power</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, new_voting_power: u64) {
    self.<a href="voting_power.md#0x2_voting_power">voting_power</a> = new_voting_power;
}
</code></pre>



</details>

<a name="0x2_validator_pending_stake_amount"></a>

## Function `pending_stake_amount`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_pending_stake_amount">pending_stake_amount</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): u64
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_pending_stake_amount">pending_stake_amount</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): u64 {
    <a href="staking_pool.md#0x2_staking_pool_pending_stake_amount">staking_pool::pending_stake_amount</a>(&self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>)
}
</code></pre>



</details>

<a name="0x2_validator_pending_stake_withdraw_amount"></a>

## Function `pending_stake_withdraw_amount`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_pending_stake_withdraw_amount">pending_stake_withdraw_amount</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): u64
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_pending_stake_withdraw_amount">pending_stake_withdraw_amount</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): u64 {
    <a href="staking_pool.md#0x2_staking_pool_pending_stake_withdraw_amount">staking_pool::pending_stake_withdraw_amount</a>(&self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>)
}
</code></pre>



</details>

<a name="0x2_validator_gas_price"></a>

## Function `gas_price`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_gas_price">gas_price</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): u64
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_gas_price">gas_price</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): u64 {
    self.gas_price
}
</code></pre>



</details>

<a name="0x2_validator_commission_rate"></a>

## Function `commission_rate`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_commission_rate">commission_rate</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): u64
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_commission_rate">commission_rate</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): u64 {
    self.commission_rate
}
</code></pre>



</details>

<a name="0x2_validator_pool_token_exchange_rate_at_epoch"></a>

## Function `pool_token_exchange_rate_at_epoch`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_pool_token_exchange_rate_at_epoch">pool_token_exchange_rate_at_epoch</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>, epoch: u64): <a href="staking_pool.md#0x2_staking_pool_PoolTokenExchangeRate">staking_pool::PoolTokenExchangeRate</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_pool_token_exchange_rate_at_epoch">pool_token_exchange_rate_at_epoch</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>, epoch: u64): PoolTokenExchangeRate {
    <a href="staking_pool.md#0x2_staking_pool_pool_token_exchange_rate_at_epoch">staking_pool::pool_token_exchange_rate_at_epoch</a>(&self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>, epoch)
}
</code></pre>



</details>

<a name="0x2_validator_staking_pool_id"></a>

## Function `staking_pool_id`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_staking_pool_id">staking_pool_id</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): <a href="object.md#0x2_object_ID">object::ID</a>
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_staking_pool_id">staking_pool_id</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>): ID {
    <a href="object.md#0x2_object_id">object::id</a>(&self.<a href="staking_pool.md#0x2_staking_pool">staking_pool</a>)
}
</code></pre>



</details>

<a name="0x2_validator_is_duplicate"></a>

## Function `is_duplicate`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_is_duplicate">is_duplicate</a>(self: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>, other: &<a href="validator.md#0x2_validator_Validator">validator::Validator</a>): bool
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_is_duplicate">is_duplicate</a>(self: &<a href="validator.md#0x2_validator_Validator">Validator</a>, other: &<a href="validator.md#0x2_validator_Validator">Validator</a>): bool {
     self.metadata.sui_address == other.metadata.sui_address
        || self.metadata.name == other.metadata.name
        || self.metadata.net_address == other.metadata.net_address
        || self.metadata.p2p_address == other.metadata.p2p_address
        || self.metadata.protocol_pubkey_bytes == other.metadata.protocol_pubkey_bytes
}
</code></pre>



</details>

<a name="0x2_validator_update_name"></a>

## Function `update_name`

Update name of the validator.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_name">update_name</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, name: <a href="_String">string::String</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_name">update_name</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, name: String) {
    self.metadata.name = name;
}
</code></pre>



</details>

<a name="0x2_validator_update_description"></a>

## Function `update_description`

Update description of the validator.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_description">update_description</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, description: <a href="_String">string::String</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_description">update_description</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, description: String) {
    self.metadata.description = description;
}
</code></pre>



</details>

<a name="0x2_validator_update_image_url"></a>

## Function `update_image_url`

Update image url of the validator.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_image_url">update_image_url</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, image_url: <a href="url.md#0x2_url_Url">url::Url</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_image_url">update_image_url</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, image_url: Url) {
    self.metadata.image_url = image_url;
}
</code></pre>



</details>

<a name="0x2_validator_update_project_url"></a>

## Function `update_project_url`

Update project url of the validator.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_project_url">update_project_url</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, project_url: <a href="url.md#0x2_url_Url">url::Url</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_project_url">update_project_url</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, project_url: Url) {
    self.metadata.project_url = project_url;
}
</code></pre>



</details>

<a name="0x2_validator_update_next_epoch_network_address"></a>

## Function `update_next_epoch_network_address`

Update network address of this validator, taking effects from next epoch


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_network_address">update_next_epoch_network_address</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, net_address: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_network_address">update_next_epoch_network_address</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, net_address: <a href="">vector</a>&lt;u8&gt;) {
    self.metadata.next_epoch_net_address = <a href="_some">option::some</a>(net_address);
    <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(&self.metadata);
}
</code></pre>



</details>

<a name="0x2_validator_update_next_epoch_p2p_address"></a>

## Function `update_next_epoch_p2p_address`

Update p2p address of this validator, taking effects from next epoch


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_p2p_address">update_next_epoch_p2p_address</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, p2p_address: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_p2p_address">update_next_epoch_p2p_address</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, p2p_address: <a href="">vector</a>&lt;u8&gt;) {
    self.metadata.next_epoch_p2p_address = <a href="_some">option::some</a>(p2p_address);
    <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(&self.metadata);
}
</code></pre>



</details>

<a name="0x2_validator_update_next_epoch_consensus_address"></a>

## Function `update_next_epoch_consensus_address`

Update consensus address of this validator, taking effects from next epoch


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_consensus_address">update_next_epoch_consensus_address</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, consensus_address: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_consensus_address">update_next_epoch_consensus_address</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, consensus_address: <a href="">vector</a>&lt;u8&gt;) {
    self.metadata.next_epoch_consensus_address = <a href="_some">option::some</a>(consensus_address);
    <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(&self.metadata);
}
</code></pre>



</details>

<a name="0x2_validator_update_next_epoch_worker_address"></a>

## Function `update_next_epoch_worker_address`

Update worker address of this validator, taking effects from next epoch


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_worker_address">update_next_epoch_worker_address</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, worker_address: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_worker_address">update_next_epoch_worker_address</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, worker_address: <a href="">vector</a>&lt;u8&gt;) {
    self.metadata.next_epoch_worker_address = <a href="_some">option::some</a>(worker_address);
    <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(&self.metadata);
}
</code></pre>



</details>

<a name="0x2_validator_update_next_epoch_protocol_pubkey"></a>

## Function `update_next_epoch_protocol_pubkey`

Update protocol public key of this validator, taking effects from next epoch


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_protocol_pubkey">update_next_epoch_protocol_pubkey</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, protocol_pubkey: <a href="">vector</a>&lt;u8&gt;, proof_of_possession: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_protocol_pubkey">update_next_epoch_protocol_pubkey</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, protocol_pubkey: <a href="">vector</a>&lt;u8&gt;, proof_of_possession: <a href="">vector</a>&lt;u8&gt;) {
    // TODO <b>move</b> proof of possession verification <b>to</b> the <b>native</b> function
    <a href="validator.md#0x2_validator_verify_proof_of_possession">verify_proof_of_possession</a>(proof_of_possession, self.metadata.sui_address, protocol_pubkey);
    self.metadata.next_epoch_protocol_pubkey_bytes = <a href="_some">option::some</a>(protocol_pubkey);
    self.metadata.next_epoch_proof_of_possession = <a href="_some">option::some</a>(proof_of_possession);
    <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(&self.metadata);
}
</code></pre>



</details>

<a name="0x2_validator_update_next_epoch_network_pubkey"></a>

## Function `update_next_epoch_network_pubkey`

Update network public key of this validator, taking effects from next epoch


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_network_pubkey">update_next_epoch_network_pubkey</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, network_pubkey: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_network_pubkey">update_next_epoch_network_pubkey</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, network_pubkey: <a href="">vector</a>&lt;u8&gt;) {
    self.metadata.next_epoch_network_pubkey_bytes = <a href="_some">option::some</a>(network_pubkey);
    <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(&self.metadata);
}
</code></pre>



</details>

<a name="0x2_validator_update_next_epoch_worker_pubkey"></a>

## Function `update_next_epoch_worker_pubkey`

Update Narwhal worker public key of this validator, taking effects from next epoch


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_worker_pubkey">update_next_epoch_worker_pubkey</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>, worker_pubkey: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_update_next_epoch_worker_pubkey">update_next_epoch_worker_pubkey</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>, worker_pubkey: <a href="">vector</a>&lt;u8&gt;) {
    self.metadata.next_epoch_worker_pubkey_bytes = <a href="_some">option::some</a>(worker_pubkey);
    <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(&self.metadata);
}
</code></pre>



</details>

<a name="0x2_validator_effectuate_staged_metadata"></a>

## Function `effectuate_staged_metadata`

Effectutate all staged next epoch metadata for this validator.
NOTE: this function SHOULD ONLY be called by validator_set when
advancing an epoch.


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_effectuate_staged_metadata">effectuate_staged_metadata</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">validator::Validator</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b>(<b>friend</b>) <b>fun</b> <a href="validator.md#0x2_validator_effectuate_staged_metadata">effectuate_staged_metadata</a>(self: &<b>mut</b> <a href="validator.md#0x2_validator_Validator">Validator</a>) {
    <b>if</b> (<a href="_is_some">option::is_some</a>(<a href="validator.md#0x2_validator_next_epoch_network_address">next_epoch_network_address</a>(self))) {
        self.metadata.net_address = <a href="_extract">option::extract</a>(&<b>mut</b> self.metadata.next_epoch_net_address);
        self.metadata.next_epoch_net_address = <a href="_none">option::none</a>();
    };

    <b>if</b> (<a href="_is_some">option::is_some</a>(<a href="validator.md#0x2_validator_next_epoch_p2p_address">next_epoch_p2p_address</a>(self))) {
        self.metadata.p2p_address = <a href="_extract">option::extract</a>(&<b>mut</b> self.metadata.next_epoch_p2p_address);
        self.metadata.next_epoch_p2p_address = <a href="_none">option::none</a>();
    };

    <b>if</b> (<a href="_is_some">option::is_some</a>(<a href="validator.md#0x2_validator_next_epoch_consensus_address">next_epoch_consensus_address</a>(self))) {
        self.metadata.consensus_address = <a href="_extract">option::extract</a>(&<b>mut</b> self.metadata.next_epoch_consensus_address);
        self.metadata.next_epoch_consensus_address = <a href="_none">option::none</a>();
    };

    <b>if</b> (<a href="_is_some">option::is_some</a>(<a href="validator.md#0x2_validator_next_epoch_worker_address">next_epoch_worker_address</a>(self))) {
        self.metadata.worker_address = <a href="_extract">option::extract</a>(&<b>mut</b> self.metadata.next_epoch_worker_address);
        self.metadata.next_epoch_worker_address = <a href="_none">option::none</a>();
    };

    <b>if</b> (<a href="_is_some">option::is_some</a>(<a href="validator.md#0x2_validator_next_epoch_protocol_pubkey_bytes">next_epoch_protocol_pubkey_bytes</a>(self))) {
        self.metadata.protocol_pubkey_bytes = <a href="_extract">option::extract</a>(&<b>mut</b> self.metadata.next_epoch_protocol_pubkey_bytes);
        self.metadata.next_epoch_protocol_pubkey_bytes = <a href="_none">option::none</a>();
        self.metadata.proof_of_possession = <a href="_extract">option::extract</a>(&<b>mut</b> self.metadata.next_epoch_proof_of_possession);
        self.metadata.next_epoch_proof_of_possession = <a href="_none">option::none</a>();
    };

    <b>if</b> (<a href="_is_some">option::is_some</a>(<a href="validator.md#0x2_validator_next_epoch_network_pubkey_bytes">next_epoch_network_pubkey_bytes</a>(self))) {
        self.metadata.network_pubkey_bytes = <a href="_extract">option::extract</a>(&<b>mut</b> self.metadata.next_epoch_network_pubkey_bytes);
        self.metadata.next_epoch_network_pubkey_bytes = <a href="_none">option::none</a>();
    };

    <b>if</b> (<a href="_is_some">option::is_some</a>(<a href="validator.md#0x2_validator_next_epoch_worker_pubkey_bytes">next_epoch_worker_pubkey_bytes</a>(self))) {
        self.metadata.worker_pubkey_bytes = <a href="_extract">option::extract</a>(&<b>mut</b> self.metadata.next_epoch_worker_pubkey_bytes);
        self.metadata.next_epoch_worker_pubkey_bytes = <a href="_none">option::none</a>();
    };
}
</code></pre>



</details>

<a name="0x2_validator_validate_metadata"></a>

## Function `validate_metadata`

Aborts if validator metadata is valid


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(metadata: &<a href="validator.md#0x2_validator_ValidatorMetadata">validator::ValidatorMetadata</a>)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_validate_metadata">validate_metadata</a>(metadata: &<a href="validator.md#0x2_validator_ValidatorMetadata">ValidatorMetadata</a>) {
    <a href="validator.md#0x2_validator_validate_metadata_bcs">validate_metadata_bcs</a>(<a href="_to_bytes">bcs::to_bytes</a>(metadata));
}
</code></pre>



</details>

<a name="0x2_validator_validate_metadata_bcs"></a>

## Function `validate_metadata_bcs`



<pre><code><b>public</b> <b>fun</b> <a href="validator.md#0x2_validator_validate_metadata_bcs">validate_metadata_bcs</a>(metadata: <a href="">vector</a>&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>public</b> <b>native</b> <b>fun</b> <a href="validator.md#0x2_validator_validate_metadata_bcs">validate_metadata_bcs</a>(metadata: <a href="">vector</a>&lt;u8&gt;);
</code></pre>



</details>

<details>
<summary>Specification</summary>



<pre><code><b>pragma</b> opaque;
<b>aborts_if</b> [abstract] <b>true</b>;
</code></pre>



</details>
