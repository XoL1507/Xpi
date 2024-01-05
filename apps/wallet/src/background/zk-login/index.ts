import { BCS } from '@mysten/bcs';
import {
    Ed25519Keypair,
    SUI_ADDRESS_LENGTH,
    type SuiAddress,
    bcs,
    normalizeSuiAddress,
    SIGNATURE_SCHEME_TO_FLAG,
} from '@mysten/sui.js';
import { blake2b } from '@noble/hashes/blake2b';
import { bytesToHex, randomBytes } from '@noble/hashes/utils';
import { toBufferBE, toBigIntBE } from 'bigint-buffer';
import { base64url, decodeJwt } from 'jose';
import Browser from 'webextension-polyfill';

import keyring from '../keyring';
import { cacheAccountCredentials } from './keys-vault';
import { getStoredZkLoginAccount, storeZkLoginAccount } from './storage';
import { getAddressSeed, poseidonHash } from './utils';

bcs.registerStructType('AddressParams', {
    iss: BCS.STRING,
    key_claim_name: BCS.STRING,
});

const clientID =
    '946731352276-pk5glcg8cqo38ndb39h7j093fpsphusu.apps.googleusercontent.com';
const redirectUri = Browser.identity.getRedirectURL();
const nonceLen = Math.ceil(256 / 6);

export type ZkProofsParams = {
    ephemeralPublicKey: bigint;
    jwt: string;
    jwtRandom: bigint;
    maxEpoch: number;
    userPin: bigint;
};
export type AuxInputs = {
    addr_seed: string;
    eph_public_key: string[];
    jwt_sha2_hash: string[];
    jwt_signature: string;
    key_claim_name: 'sub';
    masked_content: number[];
    max_epoch: number;
    num_sha2_blocks: number;
    payload_len: number;
    payload_start_index: number;
};
export type ProofPoints = {
    pi_a: string[];
    pi_b: string[][];
    pi_c: string[];
};
export type PublicInputs = string[];
export type ZkProofsResponse = {
    aux_inputs: AuxInputs;
    proof_points: ProofPoints;
    public_inputs: PublicInputs;
};

async function createZKProofs({
    ephemeralPublicKey,
    jwt,
    jwtRandom,
    maxEpoch,
    userPin,
}: ZkProofsParams): Promise<ZkProofsResponse> {
    const response = await fetch('http://185.209.177.123:8000/zkp', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify({
            eph_public_key: ephemeralPublicKey.toString(),
            jwt,
            jwt_rand: jwtRandom.toString(),
            key_claim_name: 'sub',
            max_epoch: maxEpoch,
            user_pin: userPin.toString(),
        }),
    });
    if (!response.ok) {
        throw new Error(
            `Failed to fetch proofs, ${response.status} (${response.statusText})`
        );
    }
    return response.json();
}

function getMaxEpoch(currentEpoch: number) {
    return currentEpoch + 2;
}

function generateNonce(
    ephemeralPublicKey: bigint,
    maxEpoch: number,
    randomness: bigint
) {
    const eph_public_key_0 = ephemeralPublicKey / 2n ** 128n;
    const eph_public_key_1 = ephemeralPublicKey % 2n ** 128n;
    const bignum = poseidonHash([
        eph_public_key_0,
        eph_public_key_1,
        maxEpoch,
        randomness,
    ]);
    const Z = toBufferBE(bignum, 32); // padded to 32 bytes
    const nonce = base64url.encode(Z);
    if (nonce.length !== nonceLen) {
        throw new Error(
            `Length of nonce ${nonce} (${nonce.length}) is not equal to ${nonceLen}`
        );
    }
    return nonce;
}

function prepareZKLogin(maxEpoch: number) {
    const ephemeralKeyPair = new Ed25519Keypair();
    const randomness = toBigIntBE(Buffer.from(randomBytes(16)));
    const nonce = generateNonce(
        toBigIntBE(Buffer.from(ephemeralKeyPair.getPublicKey().toBytes())),
        maxEpoch,
        randomness
    );
    return {
        ephemeralKeyPair,
        randomness,
        nonce,
    };
}

async function getAddress({
    value,
    userPin,
    iss,
}: {
    value: string;
    userPin: bigint;
    iss: string;
}) {
    const addressSeedBytes = toBufferBE(
        await getAddressSeed(value, userPin),
        32
    );
    const addressParamBytes = bcs
        .ser('AddressParams', { iss, key_claim_name: 'sub' })
        .toBytes();
    const tmp = new Uint8Array(
        1 + addressSeedBytes.length + addressParamBytes.length
    );
    tmp.set([SIGNATURE_SCHEME_TO_FLAG['zkLoginFlag']]);
    tmp.set(addressParamBytes, 1);
    tmp.set(addressSeedBytes, 1 + addressParamBytes.length);
    return normalizeSuiAddress(
        bytesToHex(blake2b(tmp, { dkLen: 32 })).slice(0, SUI_ADDRESS_LENGTH * 2)
    );
}

export async function createZkAccount(
    currentEpoch: number,
    accountPin?: string
) {
    const maxEpoch = getMaxEpoch(currentEpoch);
    const { nonce, ephemeralKeyPair, randomness } = prepareZKLogin(maxEpoch);
    const jwt = await zkLogin(nonce);
    const decodedJwt = decodeJwt(jwt);
    const userPin = accountPin
        ? BigInt(accountPin)
        : toBigIntBE(Buffer.from(randomBytes(16)));
    const proofs = await createZKProofs({
        ephemeralPublicKey: toBigIntBE(
            Buffer.from(ephemeralKeyPair.getPublicKey().toBytes())
        ),
        jwt,
        jwtRandom: randomness,
        maxEpoch,
        userPin,
    });
    if (!decodedJwt.sub || !decodedJwt.iss || !decodedJwt.email) {
        throw new Error('Missing jtw data');
    }
    const address = await getAddress({
        value: decodedJwt.sub,
        iss: decodedJwt.iss,
        userPin,
    });
    const account = {
        address,
        email: decodedJwt.email as string,
        sub: decodedJwt.sub,
        pin: userPin.toString(),
    };
    await storeZkLoginAccount(account);
    keyring.importZkAccount(account);
    await cacheAccountCredentials({ address, ephemeralKeyPair, proofs });
    return { pin: account.pin, address, email: account.email };
}

export async function zkLogin(nonce: string, loginAccount?: string) {
    const params = new URLSearchParams();
    params.append('client_id', clientID);
    params.append('response_type', 'id_token');
    params.append('redirect_uri', redirectUri);
    params.append('scope', 'openid email');
    params.append('nonce', nonce);
    // This can be used for logins after the user has already connected a google account
    // and we need to make sure that the user logged in with the correct account
    if (loginAccount) {
        params.append('login_hint', loginAccount);
    }
    const url = `https://accounts.google.com/o/oauth2/v2/auth?${params.toString()}`;
    const responseURL = new URL(
        await Browser.identity.launchWebAuthFlow({
            url,
            interactive: true,
        })
    );
    const responseParams = new URLSearchParams(
        responseURL.hash.replace('#', '')
    );
    const jwt = responseParams.get('id_token');
    if (!jwt) {
        throw new Error('JWT is missing');
    }
    return jwt;
}

export async function authenticateAccount(
    address: SuiAddress,
    currentEpoch: number
) {
    const account = await getStoredZkLoginAccount();
    if (!account || account.address !== address) {
        throw new Error(`ZK-login account ${address} not found`);
    }
    const { email, pin, sub } = account;
    const maxEpoch = getMaxEpoch(currentEpoch);
    const { nonce, ephemeralKeyPair, randomness } = prepareZKLogin(maxEpoch);
    const jwt = await zkLogin(nonce, email);
    const decodedJwt = decodeJwt(jwt);
    const userPin = BigInt(pin);
    const proofs = await createZKProofs({
        ephemeralPublicKey: toBigIntBE(
            Buffer.from(ephemeralKeyPair.getPublicKey().toBytes())
        ),
        jwt,
        jwtRandom: randomness,
        maxEpoch,
        userPin,
    });
    if (!decodedJwt.sub || !decodedJwt.iss || !decodedJwt.email) {
        throw new Error('Missing jtw data');
    }
    if (decodedJwt.sub !== sub || decodedJwt.email !== email) {
        throw new Error(
            "Authenticated google account doesn't match the current account data"
        );
    }
    await cacheAccountCredentials({ address, ephemeralKeyPair, proofs });
}
