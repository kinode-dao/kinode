import { Buffer } from 'buffer'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import React from 'react'
import ReactDOM from 'react-dom/client'

import { WagmiProvider } from 'wagmi'
import { http, createConfig } from 'wagmi'
import { optimism } from 'wagmi/chains'
import { RainbowKitProvider } from '@rainbow-me/rainbowkit';

import App from './App.tsx'

import '@rainbow-me/rainbowkit/styles.css';

globalThis.Buffer = Buffer

export const config = createConfig({
  chains: [optimism],
  transports: {
    [optimism.id]: http(),
  },
})

declare module 'wagmi' {
  interface Register {
    config: typeof config
  }
}

const queryClient = new QueryClient()

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
