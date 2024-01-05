// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
import type {
	ExecuteTransactionRequestType,
	SuiEventFilter,
	SuiTransactionBlockResponseQuery,
	Order,
	CoinMetadata,
	SuiObjectDataOptions,
	SuiTransactionBlockResponseOptions,
	SuiEvent,
	SuiObjectResponseQuery,
	TransactionFilter,
	TransactionEffects,
	Unsubscribe,
	PaginatedTransactionResponse,
	SuiMoveFunctionArgType,
	SuiMoveNormalizedFunction,
	SuiMoveNormalizedModule,
	SuiMoveNormalizedModules,
	SuiMoveNormalizedStruct,
	SuiTransactionBlockResponse,
	PaginatedEvents,
	DevInspectResults,
	PaginatedCoins,
	SuiObjectResponse,
	DelegatedStake,
	CoinBalance,
	CoinSupply,
	Checkpoint,
	CommitteeInfo,
	DryRunTransactionBlockResponse,
	SuiSystemStateSummary,
	PaginatedObjectsResponse,
	ValidatorsApy,
	MoveCallMetrics,
	ObjectRead,
	ResolvedNameServiceNames,
	ProtocolConfig,
	EpochInfo,
	EpochPage,
	CheckpointPage,
	DynamicFieldName,
	DynamicFieldPage,
	NetworkMetrics,
	AddressMetrics,
	AllEpochsAddressMetrics,
} from './types/index.js';
import {
	isValidTransactionDigest,
	isValidSuiAddress,
	isValidSuiObjectId,
	normalizeSuiAddress,
	normalizeSuiObjectId,
} from '../utils/sui-types.js';
import { fromB58, toB64, toHEX } from '@mysten/bcs';
import type { SerializedSignature } from '../cryptography/signature.js';
import type { TransactionBlock } from '../builder/index.js';
import { isTransactionBlock } from '../builder/index.js';
import { SuiHTTPTransport } from './http-transport.js';
import type { SuiTransport } from './http-transport.js';
import type { Keypair } from '../cryptography/index.js';

export interface PaginationArguments<Cursor> {
	/** Optional paging cursor */
	cursor?: Cursor;
	/** Maximum item returned per page */
	limit?: number | null;
}

export interface OrderArguments {
	order?: Order | null;
}

/**
 * Configuration options for the SuiClient
 * You must provide either a `url` or a `transport`
 */
export type SuiClientOptions = NetworkOrTransport;

export type NetworkOrTransport =
	| {
			url: string;
			transport?: never;
	  }
	| {
			transport: SuiTransport;
			url?: never;
	  };

export class SuiClient {
	protected transport: SuiTransport;
	/**
	 * Establish a connection to a Sui RPC endpoint
	 *
	 * @param options configuration options for the API Client
	 */
	constructor(options: SuiClientOptions) {
		this.transport = options.transport ?? new SuiHTTPTransport({ url: options.url });
	}

	async getRpcApiVersion(): Promise<string | undefined> {
		const resp = await this.transport.request<{ info: { version: string } }>({
			method: 'rpc.discover',
			params: [],
		});

		return resp.info.version;
	}

	/**
	 * Get all Coin<`coin_type`> objects owned by an address.
	 */
	async getCoins(
		input: {
			owner: string;
			coinType?: string | null;
		} & PaginationArguments<PaginatedCoins['nextCursor']>,
	): Promise<PaginatedCoins> {
		if (!input.owner || !isValidSuiAddress(normalizeSuiAddress(input.owner))) {
			throw new Error('Invalid Sui address');
		}

		return await this.transport.request({
			method: 'suix_getCoins',
			params: [input.owner, input.coinType, input.cursor, input.limit],
		});
	}

	/**
	 * Get all Coin objects owned by an address.
	 */
	async getAllCoins(
		input: {
			owner: string;
		} & PaginationArguments<PaginatedCoins['nextCursor']>,
	): Promise<PaginatedCoins> {
		if (!input.owner || !isValidSuiAddress(normalizeSuiAddress(input.owner))) {
			throw new Error('Invalid Sui address');
		}

		return await this.transport.request({
			method: 'suix_getAllCoins',
			params: [input.owner, input.cursor, input.limit],
		});
	}

	/**
	 * Get the total coin balance for one coin type, owned by the address owner.
	 */
	async getBalance(input: {
		owner: string;
		/** optional fully qualified type names for the coin (e.g., 0x168da5bf1f48dafc111b0a488fa454aca95e0b5e::usdc::USDC), default to 0x2::sui::SUI if not specified. */
		coinType?: string | null;
	}): Promise<CoinBalance> {
		if (!input.owner || !isValidSuiAddress(normalizeSuiAddress(input.owner))) {
			throw new Error('Invalid Sui address');
		}
		return await this.transport.request({
			method: 'suix_getBalance',
			params: [input.owner, input.coinType],
		});
	}

	/**
	 * Get the total coin balance for all coin types, owned by the address owner.
	 */
	async getAllBalances(input: { owner: string }): Promise<CoinBalance[]> {
		if (!input.owner || !isValidSuiAddress(normalizeSuiAddress(input.owner))) {
			throw new Error('Invalid Sui address');
		}
		return await this.transport.request({ method: 'suix_getAllBalances', params: [input.owner] });
	}

	/**
	 * Fetch CoinMetadata for a given coin type
	 */
	async getCoinMetadata(input: { coinType: string }): Promise<CoinMetadata | null> {
		return await this.transport.request({
			method: 'suix_getCoinMetadata',
			params: [input.coinType],
		});
	}

	/**
	 *  Fetch total supply for a coin
	 */
	async getTotalSupply(input: { coinType: string }): Promise<CoinSupply> {
		return await this.transport.request({
			method: 'suix_getTotalSupply',
			params: [input.coinType],
		});
	}

	/**
	 * Invoke any RPC method
	 * @param method the method to be invoked
	 * @param args the arguments to be passed to the RPC request
	 */
	async call<T = unknown>(method: string, params: unknown[]): Promise<T> {
		return await this.transport.request({ method, params });
	}

	/**
	 * Get Move function argument types like read, write and full access
	 */
	async getMoveFunctionArgTypes(input: {
		package: string;
		module: string;
		function: string;
	}): Promise<SuiMoveFunctionArgType[]> {
		return await this.transport.request({
			method: 'sui_getMoveFunctionArgTypes',
			params: [input.package, input.module, input.function],
		});
	}

	/**
	 * Get a map from module name to
	 * structured representations of Move modules
	 */
	async getNormalizedMoveModulesByPackage(input: {
		package: string;
	}): Promise<SuiMoveNormalizedModules> {
		return await this.transport.request({
			method: 'sui_getNormalizedMoveModulesByPackage',
			params: [input.package],
		});
	}

	/**
	 * Get a structured representation of Move module
	 */
	async getNormalizedMoveModule(input: {
		package: string;
		module: string;
	}): Promise<SuiMoveNormalizedModule> {
		return await this.transport.request({
			method: 'sui_getNormalizedMoveModule',
			params: [input.package, input.module],
		});
	}

	/**
	 * Get a structured representation of Move function
	 */
	async getNormalizedMoveFunction(input: {
		package: string;
		module: string;
		function: string;
	}): Promise<SuiMoveNormalizedFunction> {
		return await this.transport.request({
			method: 'sui_getNormalizedMoveFunction',
			params: [input.package, input.module, input.function],
		});
	}

	/**
	 * Get a structured representation of Move struct
	 */
	async getNormalizedMoveStruct(input: {
		package: string;
		module: string;
		struct: string;
	}): Promise<SuiMoveNormalizedStruct> {
		return await this.transport.request({
			method: 'sui_getNormalizedMoveStruct',
			params: [input.package, input.module, input.struct],
		});
	}

	/**
	 * Get all objects owned by an address
	 */
	async getOwnedObjects(
		input: {
			owner: string;
		} & PaginationArguments<PaginatedObjectsResponse['nextCursor']> &
			SuiObjectResponseQuery,
	): Promise<PaginatedObjectsResponse> {
		if (!input.owner || !isValidSuiAddress(normalizeSuiAddress(input.owner))) {
			throw new Error('Invalid Sui address');
		}

		return await this.transport.request({
			method: 'suix_getOwnedObjects',
			params: [
				input.owner,
				{
					filter: input.filter,
					options: input.options,
				} as SuiObjectResponseQuery,
				input.cursor,
				input.limit,
			],
		});
	}

	/**
	 * Get details about an object
	 */
	async getObject(input: {
		id: string;
		options?: SuiObjectDataOptions;
	}): Promise<SuiObjectResponse> {
		if (!input.id || !isValidSuiObjectId(normalizeSuiObjectId(input.id))) {
			throw new Error('Invalid Sui Object id');
		}
		return await this.transport.request({
			method: 'sui_getObject',
			params: [input.id, input.options],
		});
	}

	async tryGetPastObject(input: {
		id: string;
		version: number;
		options?: SuiObjectDataOptions;
	}): Promise<ObjectRead> {
		return await this.transport.request({
			method: 'sui_tryGetPastObject',
			params: [input.id, input.version, input.options],
		});
	}

	/**
	 * Batch get details about a list of objects. If any of the object ids are duplicates the call will fail
	 */
	async multiGetObjects(input: {
		ids: string[];
		options?: SuiObjectDataOptions;
	}): Promise<SuiObjectResponse[]> {
		input.ids.forEach((id) => {
			if (!id || !isValidSuiObjectId(normalizeSuiObjectId(id))) {
				throw new Error(`Invalid Sui Object id ${id}`);
			}
		});
		const hasDuplicates = input.ids.length !== new Set(input.ids).size;
		if (hasDuplicates) {
			throw new Error(`Duplicate object ids in batch call ${input.ids}`);
		}

		return await this.transport.request({
			method: 'sui_multiGetObjects',
			params: [input.ids, input.options],
		});
	}

	/**
	 * Get transaction blocks for a given query criteria
	 */
	async queryTransactionBlocks(
		input: SuiTransactionBlockResponseQuery &
			PaginationArguments<PaginatedTransactionResponse['nextCursor']> &
			OrderArguments,
	): Promise<PaginatedTransactionResponse> {
		return await this.transport.request({
			method: 'suix_queryTransactionBlocks',
			params: [
				{
					filter: input.filter,
					options: input.options,
				} as SuiTransactionBlockResponseQuery,
				input.cursor,
				input.limit,
				(input.order || 'descending') === 'descending',
			],
		});
	}

	async getTransactionBlock(input: {
		digest: string;
		options?: SuiTransactionBlockResponseOptions;
	}): Promise<SuiTransactionBlockResponse> {
		if (!isValidTransactionDigest(input.digest)) {
			throw new Error('Invalid Transaction digest');
		}
		return await this.transport.request({
			method: 'sui_getTransactionBlock',
			params: [input.digest, input.options],
		});
	}

	async multiGetTransactionBlocks(input: {
		digests: string[];
		options?: SuiTransactionBlockResponseOptions;
	}): Promise<SuiTransactionBlockResponse[]> {
		input.digests.forEach((d) => {
			if (!isValidTransactionDigest(d)) {
				throw new Error(`Invalid Transaction digest ${d}`);
			}
		});

		const hasDuplicates = input.digests.length !== new Set(input.digests).size;
		if (hasDuplicates) {
			throw new Error(`Duplicate digests in batch call ${input.digests}`);
		}

		return await this.transport.request({
			method: 'sui_multiGetTransactionBlocks',
			params: [input.digests, input.options],
		});
	}

	async executeTransactionBlock(input: {
		transactionBlock: Uint8Array | string;
		signature: SerializedSignature | SerializedSignature[];
		options?: SuiTransactionBlockResponseOptions;
		requestType?: ExecuteTransactionRequestType;
	}): Promise<SuiTransactionBlockResponse> {
		return await this.transport.request({
			method: 'sui_executeTransactionBlock',
			params: [
				typeof input.transactionBlock === 'string'
					? input.transactionBlock
					: toB64(input.transactionBlock),
				Array.isArray(input.signature) ? input.signature : [input.signature],
				input.options,
				input.requestType,
			],
		});
	}

	async signAndExecuteTransactionBlock({
		transactionBlock,
		signer,
		...input
	}: {
		transactionBlock: Uint8Array | TransactionBlock;
		signer: Keypair;
		options?: SuiTransactionBlockResponseOptions;
		requestType?: ExecuteTransactionRequestType;
	}): Promise<SuiTransactionBlockResponse> {
		let transactionBytes;

		if (transactionBlock instanceof Uint8Array) {
			transactionBytes = transactionBlock;
		} else {
			transactionBlock.setSenderIfNotSet(await signer.getPublicKey().toSuiAddress());
			transactionBytes = await transactionBlock.build({ client: this });
		}

		const { signature, bytes } = await signer.signTransactionBlock(transactionBytes);

		return this.executeTransactionBlock({
			transactionBlock: bytes,
			signature,
			...input,
		});
	}

	/**
	 * Get total number of transactions
	 */

	async getTotalTransactionBlocks(): Promise<bigint> {
		const resp = await this.transport.request<string>({
			method: 'sui_getTotalTransactionBlocks',
			params: [],
		});
		return BigInt(resp);
	}

	/**
	 * Getting the reference gas price for the network
	 */
	async getReferenceGasPrice(): Promise<bigint> {
		const resp = await this.transport.request<string>({
			method: 'suix_getReferenceGasPrice',
			params: [],
		});
		return BigInt(resp);
	}

	/**
	 * Return the delegated stakes for an address
	 */
	async getStakes(input: { owner: string }): Promise<DelegatedStake[]> {
		if (!input.owner || !isValidSuiAddress(normalizeSuiAddress(input.owner))) {
			throw new Error('Invalid Sui address');
		}
		return await this.transport.request({ method: 'suix_getStakes', params: [input.owner] });
	}

	/**
	 * Return the delegated stakes queried by id.
	 */
	async getStakesByIds(input: { stakedSuiIds: string[] }): Promise<DelegatedStake[]> {
		input.stakedSuiIds.forEach((id) => {
			if (!id || !isValidSuiObjectId(normalizeSuiObjectId(id))) {
				throw new Error(`Invalid Sui Stake id ${id}`);
			}
		});
		return await this.transport.request({
			method: 'suix_getStakesByIds',
			params: [input.stakedSuiIds],
		});
	}

	/**
	 * Return the latest system state content.
	 */
	async getLatestSuiSystemState(): Promise<SuiSystemStateSummary> {
		return await this.transport.request({ method: 'suix_getLatestSuiSystemState', params: [] });
	}

	/**
	 * Get events for a given query criteria
	 */
	async queryEvents(
		input: {
			/** the event query criteria. */
			query: SuiEventFilter;
		} & PaginationArguments<PaginatedEvents['nextCursor']> &
			OrderArguments,
	): Promise<PaginatedEvents> {
		return await this.transport.request({
			method: 'suix_queryEvents',
			params: [
				input.query,
				input.cursor,
				input.limit,
				(input.order || 'descending') === 'descending',
			],
		});
	}

	/**
	 * Subscribe to get notifications whenever an event matching the filter occurs
	 */
	async subscribeEvent(input: {
		/** filter describing the subset of events to follow */
		filter: SuiEventFilter;
		/** function to run when we receive a notification of a new event matching the filter */
		onMessage: (event: SuiEvent) => void;
	}): Promise<Unsubscribe> {
		return this.transport.subscribe({
			method: 'suix_subscribeEvent',
			unsubscribe: 'suix_unsubscribeEvent',
			params: [input.filter],
			onMessage: input.onMessage,
		});
	}

	async subscribeTransaction(input: {
		/** filter describing the subset of events to follow */
		filter: TransactionFilter;
		/** function to run when we receive a notification of a new event matching the filter */
		onMessage: (event: TransactionEffects) => void;
	}): Promise<Unsubscribe> {
		return this.transport.subscribe({
			method: 'suix_subscribeTransaction',
			unsubscribe: 'suix_unsubscribeTransaction',
			params: [input.filter],
			onMessage: input.onMessage,
		});
	}

	/**
	 * Runs the transaction block in dev-inspect mode. Which allows for nearly any
	 * transaction (or Move call) with any arguments. Detailed results are
	 * provided, including both the transaction effects and any return values.
	 */
	async devInspectTransactionBlock(input: {
		transactionBlock: TransactionBlock | string | Uint8Array;
		sender: string;
		/** Default to use the network reference gas price stored in the Sui System State object */
		gasPrice?: bigint | number | null;
		/** optional. Default to use the current epoch number stored in the Sui System State object */
		epoch?: string | null;
	}): Promise<DevInspectResults> {
		let devInspectTxBytes;
		if (isTransactionBlock(input.transactionBlock)) {
			input.transactionBlock.setSenderIfNotSet(input.sender);
			devInspectTxBytes = toB64(
				await input.transactionBlock.build({
					client: this,
					onlyTransactionKind: true,
				}),
			);
		} else if (typeof input.transactionBlock === 'string') {
			devInspectTxBytes = input.transactionBlock;
		} else if (input.transactionBlock instanceof Uint8Array) {
			devInspectTxBytes = toB64(input.transactionBlock);
		} else {
			throw new Error('Unknown transaction block format.');
		}

		return await this.transport.request({
			method: 'sui_devInspectTransactionBlock',
			params: [input.sender, devInspectTxBytes, input.gasPrice, input.epoch],
		});
	}

	/**
	 * Dry run a transaction block and return the result.
	 */
	async dryRunTransactionBlock(input: {
		transactionBlock: Uint8Array | string;
	}): Promise<DryRunTransactionBlockResponse> {
		return await this.transport.request({
			method: 'sui_dryRunTransactionBlock',
			params: [
				typeof input.transactionBlock === 'string'
					? input.transactionBlock
					: toB64(input.transactionBlock),
			],
		});
	}

	/**
	 * Return the list of dynamic field objects owned by an object
	 */
	async getDynamicFields(
		input: {
			/** The id of the parent object */
			parentId: string;
		} & PaginationArguments<DynamicFieldPage['nextCursor']>,
	): Promise<DynamicFieldPage> {
		if (!input.parentId || !isValidSuiObjectId(normalizeSuiObjectId(input.parentId))) {
			throw new Error('Invalid Sui Object id');
		}
		return await this.transport.request({
			method: 'suix_getDynamicFields',
			params: [input.parentId, input.cursor, input.limit],
		});
	}

	/**
	 * Return the dynamic field object information for a specified object
	 */
	async getDynamicFieldObject(input: {
		/** The ID of the quered parent object */
		parentId: string;
		/** The name of the dynamic field */
		name: string | DynamicFieldName;
	}): Promise<SuiObjectResponse> {
		return await this.transport.request({
			method: 'suix_getDynamicFieldObject',
			params: [input.parentId, input.name],
		});
	}

	/**
	 * Get the sequence number of the latest checkpoint that has been executed
	 */
	async getLatestCheckpointSequenceNumber(): Promise<string> {
		const resp = await this.transport.request({
			method: 'sui_getLatestCheckpointSequenceNumber',
			params: [],
		});
		return String(resp);
	}

	/**
	 * Returns information about a given checkpoint
	 */
	async getCheckpoint(input: {
		/** The checkpoint digest or sequence number */
		id: string;
	}): Promise<Checkpoint> {
		return await this.transport.request({ method: 'sui_getCheckpoint', params: [input.id] });
	}

	/**
	 * Returns historical checkpoints paginated
	 */
	async getCheckpoints(
		input: {
			/** query result ordering, default to false (ascending order), oldest record first */
			descendingOrder: boolean;
		} & PaginationArguments<CheckpointPage['nextCursor']>,
	): Promise<CheckpointPage> {
		return await this.transport.request({
			method: 'sui_getCheckpoints',
			params: [input.cursor, input?.limit, input.descendingOrder],
		});
	}

	/**
	 * Return the committee information for the asked epoch
	 */
	async getCommitteeInfo(input?: {
		/** The epoch of interest. If null, default to the latest epoch */
		epoch?: string | null;
	}): Promise<CommitteeInfo> {
		return await this.transport.request({
			method: 'suix_getCommitteeInfo',
			params: [input?.epoch],
		});
	}

	async getNetworkMetrics(): Promise<NetworkMetrics> {
		return await this.transport.request({ method: 'suix_getNetworkMetrics', params: [] });
	}

	async getAddressMetrics(): Promise<AddressMetrics> {
		return await this.transport.request({ method: 'suix_getLatestAddressMetrics', params: [] });
	}

	async getAllEpochAddressMetrics(input?: {
		descendingOrder?: boolean;
	}): Promise<AllEpochsAddressMetrics> {
		return await this.transport.request({
			method: 'suix_getAllEpochAddressMetrics',
			params: [input?.descendingOrder],
		});
	}

	/**
	 * Return the committee information for the asked epoch
	 */
	async getEpochs(
		input?: {
			descendingOrder?: boolean;
		} & PaginationArguments<EpochPage['nextCursor']>,
	): Promise<EpochPage> {
		return await this.transport.request({
			method: 'suix_getEpochs',
			params: [input?.cursor, input?.limit, input?.descendingOrder],
		});
	}

	/**
	 * Returns list of top move calls by usage
	 */
	async getMoveCallMetrics(): Promise<MoveCallMetrics> {
		return await this.transport.request({ method: 'suix_getMoveCallMetrics', params: [] });
	}

	/**
	 * Return the committee information for the asked epoch
	 */
	async getCurrentEpoch(): Promise<EpochInfo> {
		return await this.transport.request({ method: 'suix_getCurrentEpoch', params: [] });
	}

	/**
	 * Return the Validators APYs
	 */
	async getValidatorsApy(): Promise<ValidatorsApy> {
		return await this.transport.request({ method: 'suix_getValidatorsApy', params: [] });
	}

	// TODO: Migrate this to `sui_getChainIdentifier` once it is widely available.
	async getChainIdentifier(): Promise<string> {
		const checkpoint = await this.getCheckpoint({ id: '0' });
		const bytes = fromB58(checkpoint.digest);
		return toHEX(bytes.slice(0, 4));
	}

	async resolveNameServiceAddress(input: { name: string }): Promise<string | null> {
		return await this.transport.request({
			method: 'suix_resolveNameServiceAddress',
			params: [input.name],
		});
	}

	async resolveNameServiceNames(
		input: {
			address: string;
		} & PaginationArguments<ResolvedNameServiceNames['nextCursor']>,
	): Promise<ResolvedNameServiceNames> {
		return await this.transport.request({
			method: 'suix_resolveNameServiceNames',
			params: [input.address],
		});
	}

	async getProtocolConfig(input?: { version?: string }): Promise<ProtocolConfig> {
		return await this.transport.request({
			method: 'sui_getProtocolConfig',
			params: [input?.version],
		});
	}

	/**
	 * Wait for a transaction block result to be available over the API.
	 * This can be used in conjunction with `executeTransactionBlock` to wait for the transaction to
	 * be available via the API.
	 * This currently polls the `getTransactionBlock` API to check for the transaction.
	 */
	async waitForTransactionBlock({
		signal,
		timeout = 60 * 1000,
		pollInterval = 2 * 1000,
		...input
	}: {
		/** An optional abort signal that can be used to cancel */
		signal?: AbortSignal;
		/** The amount of time to wait for a transaction block. Defaults to one minute. */
		timeout?: number;
		/** The amount of time to wait between checks for the transaction block. Defaults to 2 seconds. */
		pollInterval?: number;
	} & Parameters<SuiClient['getTransactionBlock']>[0]): Promise<SuiTransactionBlockResponse> {
		const timeoutSignal = AbortSignal.timeout(timeout);
		const timeoutPromise = new Promise((_, reject) => {
			timeoutSignal.addEventListener('abort', () => reject(timeoutSignal.reason));
		});

		timeoutPromise.catch(() => {
			// Swallow unhandled rejections that might be thrown after early return
		});

		while (!timeoutSignal.aborted) {
			signal?.throwIfAborted();
			try {
				return await this.getTransactionBlock(input);
			} catch (e) {
				// Wait for either the next poll interval, or the timeout.
				await Promise.race([
					new Promise((resolve) => setTimeout(resolve, pollInterval)),
					timeoutPromise,
				]);
			}
		}

		timeoutSignal.throwIfAborted();

		// This should never happen, because the above case should always throw, but just adding it in the event that something goes horribly wrong.
		throw new Error('Unexpected error while waiting for transaction block.');
	}
}
