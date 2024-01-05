// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  createContext,
  FC,
  ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type {
  SuiAddress,
  SuiTransactionResponse,
  SignableTransaction,
} from "@mysten/sui.js";
import { WalletAdapter, WalletAdapterList } from "@mysten/wallet-adapter-base";
import { useWalletAdapters } from "./useWalletAdapters";

const DEFAULT_STORAGE_KEY = "preferredSuiWallet";

export enum WalletConnectionStatus {
  DISCONNECTED = "DISCONNECTED",
  CONNECTING = "CONNECTING",
  CONNECTED = "CONNECTED",
  // TODO: Figure out if this is really a separate status, or is just a piece of state alongside the `disconnected` state:
  ERROR = "ERROR",
}

export interface WalletContextState {
  adapters: WalletAdapterList;
  wallets: WalletAdapter[];

  // Wallet that we are currently connected to
  wallet: WalletAdapter | null;

  status: WalletConnectionStatus;
  connecting: boolean;
  connected: boolean;
  isError: boolean;

  select(walletName: string): void;
  disconnect(): Promise<void>;

  getAccounts: () => Promise<SuiAddress[]>;

  signAndExecuteTransaction(
    transaction: SignableTransaction
  ): Promise<SuiTransactionResponse>;
}

export const WalletContext = createContext<WalletContextState | null>(null);

// TODO: Add storage adapter interface
// TODO: Add storage key option
export interface WalletProviderProps {
  children: ReactNode;
  adapters: WalletAdapterList;
  autoConnect?: boolean;
}

export const WalletProvider: FC<WalletProviderProps> = ({
  children,
  adapters,
  autoConnect = true,
}) => {
  const wallets = useWalletAdapters(adapters);

  const [status, setStatus] = useState(WalletConnectionStatus.DISCONNECTED);
  const [wallet, setWallet] = useState<WalletAdapter | null>(null);

  const connected = status === WalletConnectionStatus.CONNECTED;
  const connecting = status === WalletConnectionStatus.CONNECTING;
  const isError = status === WalletConnectionStatus.ERROR;

  const disconnect = useCallback(async () => {
    wallet?.disconnect();
    setStatus(WalletConnectionStatus.DISCONNECTED);
    setWallet(null);
    localStorage.removeItem(DEFAULT_STORAGE_KEY);
  }, []);

  // Once we connect, we remember that we've connected before to enable auto-connect:
  useEffect(() => {
    if (connected && wallet) {
      localStorage.setItem(DEFAULT_STORAGE_KEY, wallet.name);
    }
  }, [wallet, connected]);

  const select = useCallback(
    async (name: string) => {
      let selectedWallet =
        wallets.find((wallet) => wallet.name === name) ?? null;

      setWallet(selectedWallet);

      if (selectedWallet && !selectedWallet.connecting) {
        try {
          setStatus(WalletConnectionStatus.CONNECTING);
          await selectedWallet.connect();
          setStatus(WalletConnectionStatus.CONNECTED);
        } catch (e) {
          console.log("Wallet connection error", e);
          setStatus(WalletConnectionStatus.ERROR);
        }
      } else {
        setStatus(WalletConnectionStatus.DISCONNECTED);
      }
    },
    [wallets]
  );

  // Auto-connect to the preferred wallet if there is one in storage:
  useEffect(() => {
    if (!wallet && !connected && !connecting && autoConnect) {
      let preferredWallet = localStorage.getItem(DEFAULT_STORAGE_KEY);
      if (typeof preferredWallet === "string") {
        select(preferredWallet);
      }
    }
  }, [wallet, connected, connecting, select, autoConnect]);

  const walletContext = useMemo<WalletContextState>(
    () => ({
      adapters,
      wallets,
      wallet,
      status,
      connecting,
      connected,
      isError,
      select,
      disconnect,

      async getAccounts() {
        if (wallet == null) throw Error("Wallet Not Connected");
        return wallet.getAccounts();
      },

      async signAndExecuteTransaction(transaction) {
        if (wallet == null) {
          throw new Error("Wallet Not Connected");
        }
        if (!wallet.signAndExecuteTransaction) {
          throw new Error(
            'Wallet does not support "signAndExecuteTransaction" method'
          );
        }
        return wallet.signAndExecuteTransaction(transaction);
      },
    }),
    [
      wallets,
      adapters,
      wallet,
      select,
      disconnect,
      connecting,
      connected,
      status,
      isError,
    ]
  );

  return (
    <WalletContext.Provider value={walletContext}>
      {children}
    </WalletContext.Provider>
  );
};

export function useWallet(): WalletContextState {
  const context = useContext(WalletContext);

  if (!context) {
    throw new Error(
      "You tried to access the `WalletContext` outside of the `WalletProvider`."
    );
  }

  return context;
}
