import { defineConfig } from 'vite'
import { nodePolyfills } from 'vite-plugin-node-polyfills'
import react from '@vitejs/plugin-react'

/*
If you are developing a UI outside of a Kinode project,
comment out the following 2 lines:
*/
import manifest from '../pkg/manifest.json'
import metadata from '../metadata.json'

/*
IMPORTANT:
This must match the process name from pkg/manifest.json + pkg/metadata.json
The format is "/" + "process_name:package_name:publisher_node"
*/
const BASE_URL = `/main:app_store:sys`;

// This is the proxy URL, it must match the node you are developing against
const PROXY_URL = (process.env.VITE_NODE_URL || 'http://127.0.0.1:8080').replace('localhost', '127.0.0.1');

console.log('process.env.VITE_NODE_URL', process.env.VITE_NODE_URL, PROXY_URL);

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
    open: true,
    proxy: {
      '^/our\\.js': {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          console.log('Rewriting path for our.js:', path);
          return '/our.js';
        },
      },
      '^/kinode\\.css': {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          console.log('Rewriting path for kinode.css:', path);
          return '/kinode.css';
        },
      },
      [`^${BASE_URL}/(?!(@vite/client|src/.*|node_modules/.*|@react-refresh|$))`]: {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          console.log('Rewriting path for other requests:', path);
          return path.replace(BASE_URL, '');
        },
      },
    },


  },
});
