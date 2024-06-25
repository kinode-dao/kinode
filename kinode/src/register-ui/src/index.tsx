import React from 'react';
import ReactDOM from 'react-dom/client'

import App from './App';
// import '@unocss/reset/tailwind.css'
// import '@rainbow-me/rainbowkit/styles.css';

import 'uno.css'
import './index.css';

import {
  getDefaultConfig,
  RainbowKitProvider,
} from '@rainbow-me/rainbowkit';
import { WagmiProvider, http } from 'wagmi';
import {
  optimism,
  anvil
} from 'wagmi/chains';
import {
  QueryClientProvider,
  QueryClient,
} from "@tanstack/react-query";


import { Buffer } from 'buffer';
window.Buffer = Buffer;


const config = getDefaultConfig({
  appName: 'Kinode Register UI',
  projectId: 'YOUR_PROJECT_ID', // apparently need project_Id if using wallet_connect
  chains: [anvil], // change back to OP main once ready
  ssr: false, // If your dApp uses server side rendering (SSR)
  transports: {
    [anvil.id]: http("http://localhost:8545")
  }
});

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WagmiProvider config={config}>
      <QueryClientProvider client={queryClient}>
        <RainbowKitProvider>
          <App />
        </RainbowKitProvider>
      </QueryClientProvider>
    </WagmiProvider>
  </React.StrictMode>,
)
