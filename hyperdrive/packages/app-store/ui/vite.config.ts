import { defineConfig, ViteDevServer } from 'vite'
import { nodePolyfills } from 'vite-plugin-node-polyfills'
import react from '@vitejs/plugin-react'

/*
If you are developing a UI outside of a Hyperware project,
comment out the following 2 lines:
*/
import manifest from '../pkg/manifest.json'
import metadata from '../metadata.json'

/*
IMPORTANT:
This must match the process name from pkg/manifest.json + pkg/metadata.json
The format is "/" + "process_name:package_name:publisher_node"
*/
const BASE_URL = `/main:app-store:sys`;

// This is the proxy URL, it must match the node you are developing against
const PROXY_URL = (process.env.VITE_NODE_URL || 'http://localhost:8080').replace(/\/$/, '');

const DEV_SERVER_PORT = 3000; // Hardcoded port for the dev server...

console.log('process.env.VITE_NODE_URL', process.env.VITE_NODE_URL, PROXY_URL);

const openUrl = `${PROXY_URL.replace(/:\d+$/, '')}:${DEV_SERVER_PORT}${BASE_URL}`;
console.log('Server will run at:', openUrl);

export default defineConfig({
  plugins: [
    nodePolyfills({
      globals: {
        Buffer: true,
      }
    }),
    react(),
  ],
  base: BASE_URL,
  build: {
    rollupOptions: {
      external: ['/our.js']
    }
  },
  server: {
    open: openUrl,
    port: DEV_SERVER_PORT,
    proxy: {
      [`^${BASE_URL}/our.js`]: {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          console.log('Proxying  jsrequest:', path);
          return '/our.js';
        },
      },
      [`^${BASE_URL}/hyperware.css`]: {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          console.log('Proxying  csrequest:', path);
          return '/hyperware.css';
        },
      },
      // This route will match all other HTTP requests to the backend
      [`^${BASE_URL}/(?!(@vite/client|src/.*|node_modules/.*|@react-refresh|$))`]: {
        target: PROXY_URL,
        changeOrigin: true,
      },

    },


  },
});
