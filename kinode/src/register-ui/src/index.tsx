import React from 'react';
import ReactDOM from 'react-dom/client'

import App from './App';
import '@rainbow-me/rainbowkit/styles.css';
import './index.css';
import './kinode.css';

import {
  getDefaultConfig,
  RainbowKitProvider,
} from '@rainbow-me/rainbowkit';
import { WagmiProvider, http } from 'wagmi';
import {
  optimism,
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
  appName: 'Kinode Register UI',
  projectId: 'KINODE_REGISTER',
  chains: [optimism],
  ssr: false,
  transports: {
    [anvil.id]: http(),
    [optimism.id]: http(),
    [mainnet.id]: http(),
  }
});

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WagmiProvider config={config}>
      <QueryClientProvider client={queryClient}>
        <RainbowKitProvider showRecentTransactions={true}>
          <App />
        </RainbowKitProvider>
      </QueryClientProvider>
    </WagmiProvider>
  </React.StrictMode>,
)
