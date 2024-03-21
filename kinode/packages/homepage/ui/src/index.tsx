import React from 'react';
import ReactDOM from 'react-dom/client';
import './index.css';
import App from './App';
import { Web3ReactProvider, Web3ReactHooks } from '@web3-react/core';
import { hooks as metaMaskHooks, metaMask } from './connectors/metamask'
import type { MetaMask } from '@web3-react/metamask'

const connectors: [MetaMask, Web3ReactHooks][] = [
  [metaMask, metaMaskHooks],
]
const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);
root.render(
  <React.StrictMode>
    <Web3ReactProvider connectors={connectors}>
        <div id="signup-page" className="col">
          <App />
        </div>
    </Web3ReactProvider>
  </React.StrictMode>
);
