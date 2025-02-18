import React from 'react';
import ReactDOM from 'react-dom/client'

import App from './App';
import '@rainbow-me/rainbowkit/styles.css';
import './index.css';
import './hyperware.css';

import {
  getDefaultConfig,
  RainbowKitProvider,
} from '@rainbow-me/rainbowkit';
import { WagmiProvider, http } from 'wagmi';
import {
  base,
  anvil,
  mainnet
} from 'wagmi/chains';
import {
  QueryClientProvider,
  QueryClient,
} from "@tanstack/react-query";

import { Buffer } from 'buffer';
window.Buffer = Buffer;

const config = getDefaultConfig({
  appName: 'Hyperdrive Register UI',
  projectId: 'c6da298e8ee4e4b00ea32cd4c20c40af',
  chains: [base],
  ssr: false,
  transports: {
    [anvil.id]: http(),
    [base.id]: http(),
    [mainnet.id]: http(),
  }
});

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WagmiProvider config={config}>
      <QueryClientProvider client={queryClient}>
        <RainbowKitProvider modalSize="compact" showRecentTransactions={true}>
          <App />
        </RainbowKitProvider>
      </QueryClientProvider>
    </WagmiProvider>
  </React.StrictMode>,
)
