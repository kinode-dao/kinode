/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_APP_TITLE: string;
  readonly REACT_APP_MAINNET_RPC_URL: string;
  readonly REACT_APP_SEPOLIA_RPC_URL: string;
  readonly VITE_NODE_URL: string;
  // Add other environment variables as needed
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}