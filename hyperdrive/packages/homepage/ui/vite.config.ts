import { defineConfig } from 'vite'
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
const BASE_URL = `/`;

// This is the proxy URL, it must match the node you are developing against
const PROXY_URL = (process.env.VITE_NODE_URL || 'http://127.0.0.1:8080').replace('localhost', '127.0.0.1');

export default defineConfig({
  plugins: [
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
          return '/our.js';
        },
      },
      '^/hyperware\\.css': {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          return '/hyperware.css';
        },
      },
      '^/version': {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          return '/version';
        },
      },
      '^/apps': {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          return '/apps';
        },
      },
      '^/favorite': {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          return '/favorite';
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
