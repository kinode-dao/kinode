import React, { useEffect, useState } from "react";
import { BrowserRouter as Router, Route, Routes } from "react-router-dom";
import { Web3ReactProvider, Web3ReactHooks } from '@web3-react/core';
import type { MetaMask } from '@web3-react/metamask'

import { PackageStore, PackageStore__factory } from "./abis/types";
import StorePage from "./pages/StorePage";
import MyAppsPage from "./pages/MyAppsPage";
import AppPage from "./pages/AppPage";
import { APP_DETAILS_PATH, MY_APPS_PATH, PUBLISH_PATH, STORE_PATH } from "./constants/path";
import { ChainId, PACKAGE_STORE_ADDRESSES } from "./constants/chain";
import PublishPage from "./pages/PublishPage";
import { hooks as metaMaskHooks, metaMask } from './utils/metamask'

const connectors: [MetaMask, Web3ReactHooks][] = [
  [metaMask, metaMaskHooks],
]

declare global {
  interface ImportMeta {
    env: {
      VITE_OPTIMISM_RPC_URL: string;
      VITE_SEPOLIA_RPC_URL: string;
      BASE_URL: string;
      VITE_NODE_URL?: string;
      DEV: boolean;
    };
  }
  interface Window {
    our: {
      node: string;
      process: string;
    };
  }
}

const {
  useProvider,
} = metaMaskHooks;

const RPC_URL = import.meta.env.VITE_OPTIMISM_RPC_URL;
const BASE_URL = import.meta.env.BASE_URL;
if (window.our) window.our.process = BASE_URL?.replace("/", "");

const PROXY_TARGET = `${import.meta.env.VITE_NODE_URL || "http://localhost:8080"
  }${BASE_URL}`;

// This env also has BASE_URL which should match the process + package name
const WEBSOCKET_URL = import.meta.env.DEV // eslint-disable-line
  ? `${PROXY_TARGET.replace("http", "ws")}`
  : undefined;

function App() {
  const provider = useProvider();
  const [nodeConnected, setNodeConnected] = useState(true); // eslint-disable-line

  const [packageAbi, setPackageAbi] = useState<PackageStore | undefined>(undefined);


  useEffect(() => {
    if (!provider) return;

    const updatePackageAbi = async () => {
      const network = await provider.getNetwork();
      if (network.chainId === ChainId.OPTIMISM) {
        setPackageAbi(PackageStore__factory.connect(
          PACKAGE_STORE_ADDRESSES[ChainId.OPTIMISM],
          provider.getSigner())
        );
      }
    };

    updatePackageAbi();

  }, [provider])

  useEffect(() => {
    // if (window.our?.node && window.our?.process) {
    //   const api = new KinodeClientApi({
    //     uri: WEBSOCKET_URL,
    //     nodeId: window.our.node,
    //     processId: window.our.process,
    //     onOpen: (_event, _api) => {
    //       console.log("Connected to Kinode");
    //       // api.send({ data: "Hello World" });
    //     },
    //     onMessage: (json, _api) => {
    //       console.log('UNEXPECTED WEBSOCKET MESSAGE', json)
    //     },
    //   });

    //   setApi(api);
    // } else {
    //   setNodeConnected(false);
    // }
  }, []);

  if (!nodeConnected) {
    return (
      <div className="flex flex-col c">
        <h2 style={{ color: "red" }}>Node not connected</h2>
        <h4>
          You need to start a node at {PROXY_TARGET} before you can use this UI
          in development.
        </h4>
      </div>
    );
  }

  const props = { provider, packageAbi };

  return (
    <div className="flex flex-col c h-screen w-screen">
      <Web3ReactProvider connectors={connectors}>
        <Router basename={BASE_URL}>
          <Routes>
            <Route path={STORE_PATH} element={<StorePage />} />
            <Route path={MY_APPS_PATH} element={<MyAppsPage />} />
            <Route path={`${APP_DETAILS_PATH}/:id`} element={<AppPage />} />
            <Route path={PUBLISH_PATH} element={<PublishPage {...props} />} />
          </Routes>
        </Router>
      </Web3ReactProvider>
    </div>
  );
}

export default App;
