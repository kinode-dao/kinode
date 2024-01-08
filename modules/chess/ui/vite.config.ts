import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

/*
If you are developing a UI outside of an Uqbar project,
comment out the following 2 lines:
*/
import manifest from '../pkg/manifest.json'
import metadata from '../pkg/metadata.json'

/*
IMPORTANT:
This must match the process name from pkg/manifest.json + pkg/metadata.json
The format is "/" + "process_name:package_name:publisher_node"
*/
const BASE_URL = `/${manifest[0].process_name}:${metadata.package}:${metadata.publisher}`;

// This is the proxy URL, it must match the node you are developing against
const PROXY_URL = (process.env.VITE_NODE_URL || 'http://127.0.0.1:8080').replace('localhost', '127.0.0.1');

console.log('process.env.VITE_NODE_URL', process.env.VITE_NODE_URL, PROXY_URL);

export default defineConfig({
  plugins: [react()],
  base: BASE_URL,
  build: {
    rollupOptions: {
      external: ['/our.js']
    }
  },
  server: {
    open: true,
    proxy: {
      '/our': {
        target: PROXY_URL,
        changeOrigin: true,
      },
      [`${BASE_URL}/our.js`]: {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => path.replace(BASE_URL, ''),
      },
      // This route will match all other HTTP requests to the backend
      [`^${BASE_URL}/(?!(@vite/client|src/.*|node_modules/.*|@react-refresh|$))`]: {
        target: PROXY_URL,
        changeOrigin: true,
      },
      // '/example': {
      //   target: PROXY_URL,
      //   changeOrigin: true,
      //   rewrite: (path) => path.replace(BASE_URL, ''),
      // // This is only for debugging purposes
      //   configure: (proxy, _options) => {
      //     proxy.on('error', (err, _req, _res) => {
      //       console.log('proxy error', err);
      //     });
      //     proxy.on('proxyReq', (proxyReq, req, _res) => {
      //       console.log('Sending Request to the Target:', req.method, req.url);
      //     });
      //     proxy.on('proxyRes', (proxyRes, req, _res) => {
      //       console.log('Received Response from the Target:', proxyRes.statusCode, req.url);
      //     });
      //   },
      // },
    }
  }
});
