import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { Web3ReactProvider, Web3ReactHooks } from '@web3-react/core';
import { hooks as metaMaskHooks, metaMask } from './connectors/metamask'
import type { MetaMask } from '@web3-react/metamask'
import '@unocss/reset/tailwind.css'
import 'uno.css'
import './index.css';

import { Buffer } from 'buffer';
window.Buffer = Buffer;

const connectors: [MetaMask, Web3ReactHooks][] = [
  [metaMask, metaMaskHooks],
]
const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);
root.render(
  <React.StrictMode>
    <Web3ReactProvider connectors={connectors}>
      <div id="signup-page" className="flex flex-col place-items-center place-content-center h-screen w-screen">
        <App />
      </div>
    </Web3ReactProvider>
  </React.StrictMode>
);
