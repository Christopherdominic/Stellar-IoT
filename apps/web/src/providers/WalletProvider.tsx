'use client';

import React, { createContext, useContext, useState, useEffect, ReactNode } from 'react';
import { requestAccess, signTransaction } from '@stellar/freighter-api';
import { Horizon } from '@stellar/stellar-sdk';

interface WalletContextType {
  publicKey: string | null;
  isConnected: boolean;
  balance: string;
  loading: boolean;
  error: string | null;
  connect: () => Promise<void>;
  disconnect: () => void;
  signTransaction: (xdr: string) => Promise<string>;
}

const WalletContext = createContext<WalletContextType | undefined>(undefined);

// Use Stellar testnet server
const server = new Horizon.Server('https://horizon-testnet.stellar.org');

// freighter-api's requestAccess() has no internal timeout: if the extension
// isn't installed, is disabled, or its content script otherwise never
// responds, the promise hangs forever with no error. Race it against our
// own timeout so the UI fails loudly instead of spinning indefinitely.
const CONNECT_TIMEOUT_MS = 8000;

function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) => setTimeout(() => reject(new Error(message)), ms)),
  ]);
}

export const WalletProvider = ({ children }: { children: ReactNode }) => {
  const [publicKey, setPublicKey] = useState<string | null>(null);
  const [balance, setBalance] = useState<string>('0');
  const [isConnected, setIsConnected] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchBalance = async (address: string) => {
    try {
      const account = await server.loadAccount(address);
      const nativeBalance = account.balances.find(b => b.asset_type === 'native');
      const balanceAmount = nativeBalance ? nativeBalance.balance : '0';
      setBalance(balanceAmount);
      setError(null);
    } catch (err) {
      console.error('Failed to fetch balance:', err);
      // If account doesn't exist on testnet, show 0 balance
      setBalance('0');
    }
  };

  const connect = async () => {
    try {
      setLoading(true);
      setError(null);
      
      // requestAccess() (not getAddress()) is what actually triggers Freighter's
      // permission popup on first connect — getAddress() only succeeds if the
      // site has already been granted access.
      const addressObj = await withTimeout(
        requestAccess(),
        CONNECT_TIMEOUT_MS,
        'Freighter did not respond. Make sure the extension is installed, enabled, and unlocked, then try again.'
      );
      
      if (addressObj.error) {
        throw new Error(addressObj.error as string);
      }

      const key = addressObj.address;
      setPublicKey(key);
      setIsConnected(true);
      
      // Fetch the actual balance
      await fetchBalance(key);
      
      localStorage.setItem('stellarPublicKey', key);
    } catch (err: any) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to connect wallet';
      setError(errorMessage);
      console.error('Wallet connection error:', err);
      alert(errorMessage);
    } finally {
      setLoading(false);
    }
  };

  const disconnect = () => {
    setPublicKey(null);
    setIsConnected(false);
    setBalance('0');
    setError(null);
    localStorage.removeItem('stellarPublicKey');
  };

  const handleSignTransaction = async (xdr: string): Promise<string> => {
    try {
      const result = await signTransaction(xdr, {
        networkPassphrase: 'Test SDF Network ; September 2015',
      });
      
      if (result.error) {
        throw new Error(result.error as string);
      }
      
      return result.signedTxXdr as string;
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to sign transaction';
      setError(errorMessage);
      throw err;
    }
  };

  // Load wallet from localStorage on mount
  useEffect(() => {
    const savedKey = localStorage.getItem('stellarPublicKey');
    if (savedKey) {
      setPublicKey(savedKey);
      setIsConnected(true);
      fetchBalance(savedKey);
    }
  }, []);

  // Poll balance periodically when connected
  useEffect(() => {
    if (!isConnected || !publicKey) return;

    const interval = setInterval(() => {
      fetchBalance(publicKey);
    }, 30000); // Update every 30 seconds

    return () => clearInterval(interval);
  }, [isConnected, publicKey]);

  return (
    <WalletContext.Provider value={{ 
      publicKey, 
      isConnected, 
      balance, 
      loading, 
      error, 
      connect, 
      disconnect,
      signTransaction: handleSignTransaction
    }}>
      {children}
    </WalletContext.Provider>
  );
};

export const useWallet = () => {
  const context = useContext(WalletContext);
  if (context === undefined) {
    throw new Error('useWallet must be used within a WalletProvider');
  }
  return context;
};