// vite.config.ts
import { defineConfig } from "file:///Users/korin/mars/kinode/kinode/packages/app_store/ui/node_modules/vite/dist/node/index.js";
import { nodePolyfills } from "file:///Users/korin/mars/kinode/kinode/packages/app_store/ui/node_modules/vite-plugin-node-polyfills/dist/index.js";
import react from "file:///Users/korin/mars/kinode/kinode/packages/app_store/ui/node_modules/@vitejs/plugin-react/dist/index.mjs";
var BASE_URL = `/main:app_store:sys`;
var PROXY_URL = (process.env.VITE_NODE_URL || "http://127.0.0.1:8080").replace("localhost", "127.0.0.1");
console.log("process.env.VITE_NODE_URL", process.env.VITE_NODE_URL, PROXY_URL);
var vite_config_default = defineConfig({
  plugins: [
    nodePolyfills({
      globals: {
        Buffer: true
      }
    }),
    react()
  ],
  base: BASE_URL,
  build: {
    rollupOptions: {
      external: ["/our.js"]
    }
  },
  server: {
    open: true,
    proxy: {
      [`^${BASE_URL}/our.js`]: {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          console.log("Proxying  jsrequest:", path);
          return "/our.js";
        }
      },
      [`^${BASE_URL}/kinode.css`]: {
        target: PROXY_URL,
        changeOrigin: true,
        rewrite: (path) => {
          console.log("Proxying  csrequest:", path);
          return "/kinode.css";
        }
      },
      // This route will match all other HTTP requests to the backend
      [`^${BASE_URL}/(?!(@vite/client|src/.*|node_modules/.*|@react-refresh|$))`]: {
        target: PROXY_URL,
        changeOrigin: true
      }
    }
  }
});
export {
  vite_config_default as default
};
//# sourceMappingURL=data:application/json;base64,ewogICJ2ZXJzaW9uIjogMywKICAic291cmNlcyI6IFsidml0ZS5jb25maWcudHMiXSwKICAic291cmNlc0NvbnRlbnQiOiBbImNvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9kaXJuYW1lID0gXCIvVXNlcnMva29yaW4vbWFycy9raW5vZGUva2lub2RlL3BhY2thZ2VzL2FwcF9zdG9yZS91aVwiO2NvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9maWxlbmFtZSA9IFwiL1VzZXJzL2tvcmluL21hcnMva2lub2RlL2tpbm9kZS9wYWNrYWdlcy9hcHBfc3RvcmUvdWkvdml0ZS5jb25maWcudHNcIjtjb25zdCBfX3ZpdGVfaW5qZWN0ZWRfb3JpZ2luYWxfaW1wb3J0X21ldGFfdXJsID0gXCJmaWxlOi8vL1VzZXJzL2tvcmluL21hcnMva2lub2RlL2tpbm9kZS9wYWNrYWdlcy9hcHBfc3RvcmUvdWkvdml0ZS5jb25maWcudHNcIjtpbXBvcnQgeyBkZWZpbmVDb25maWcgfSBmcm9tICd2aXRlJ1xuaW1wb3J0IHsgbm9kZVBvbHlmaWxscyB9IGZyb20gJ3ZpdGUtcGx1Z2luLW5vZGUtcG9seWZpbGxzJ1xuaW1wb3J0IHJlYWN0IGZyb20gJ0B2aXRlanMvcGx1Z2luLXJlYWN0J1xuXG4vKlxuSWYgeW91IGFyZSBkZXZlbG9waW5nIGEgVUkgb3V0c2lkZSBvZiBhIEtpbm9kZSBwcm9qZWN0LFxuY29tbWVudCBvdXQgdGhlIGZvbGxvd2luZyAyIGxpbmVzOlxuKi9cbmltcG9ydCBtYW5pZmVzdCBmcm9tICcuLi9wa2cvbWFuaWZlc3QuanNvbidcbmltcG9ydCBtZXRhZGF0YSBmcm9tICcuLi9tZXRhZGF0YS5qc29uJ1xuXG4vKlxuSU1QT1JUQU5UOlxuVGhpcyBtdXN0IG1hdGNoIHRoZSBwcm9jZXNzIG5hbWUgZnJvbSBwa2cvbWFuaWZlc3QuanNvbiArIHBrZy9tZXRhZGF0YS5qc29uXG5UaGUgZm9ybWF0IGlzIFwiL1wiICsgXCJwcm9jZXNzX25hbWU6cGFja2FnZV9uYW1lOnB1Ymxpc2hlcl9ub2RlXCJcbiovXG5jb25zdCBCQVNFX1VSTCA9IGAvbWFpbjphcHBfc3RvcmU6c3lzYDtcblxuLy8gVGhpcyBpcyB0aGUgcHJveHkgVVJMLCBpdCBtdXN0IG1hdGNoIHRoZSBub2RlIHlvdSBhcmUgZGV2ZWxvcGluZyBhZ2FpbnN0XG5jb25zdCBQUk9YWV9VUkwgPSAocHJvY2Vzcy5lbnYuVklURV9OT0RFX1VSTCB8fCAnaHR0cDovLzEyNy4wLjAuMTo4MDgwJykucmVwbGFjZSgnbG9jYWxob3N0JywgJzEyNy4wLjAuMScpO1xuXG5jb25zb2xlLmxvZygncHJvY2Vzcy5lbnYuVklURV9OT0RFX1VSTCcsIHByb2Nlc3MuZW52LlZJVEVfTk9ERV9VUkwsIFBST1hZX1VSTCk7XG5cbmV4cG9ydCBkZWZhdWx0IGRlZmluZUNvbmZpZyh7XG4gIHBsdWdpbnM6IFtcbiAgICBub2RlUG9seWZpbGxzKHtcbiAgICAgIGdsb2JhbHM6IHtcbiAgICAgICAgQnVmZmVyOiB0cnVlLFxuICAgICAgfVxuICAgIH0pLFxuICAgIHJlYWN0KCksXG4gIF0sXG4gIGJhc2U6IEJBU0VfVVJMLFxuICBidWlsZDoge1xuICAgIHJvbGx1cE9wdGlvbnM6IHtcbiAgICAgIGV4dGVybmFsOiBbJy9vdXIuanMnXVxuICAgIH1cbiAgfSxcbiAgc2VydmVyOiB7XG4gICAgb3BlbjogdHJ1ZSxcbiAgICBwcm94eToge1xuICAgICAgW2BeJHtCQVNFX1VSTH0vb3VyLmpzYF06IHtcbiAgICAgICAgdGFyZ2V0OiBQUk9YWV9VUkwsXG4gICAgICAgIGNoYW5nZU9yaWdpbjogdHJ1ZSxcbiAgICAgICAgcmV3cml0ZTogKHBhdGgpID0+IHtcbiAgICAgICAgICBjb25zb2xlLmxvZygnUHJveHlpbmcgIGpzcmVxdWVzdDonLCBwYXRoKTtcbiAgICAgICAgICByZXR1cm4gJy9vdXIuanMnO1xuICAgICAgICB9LFxuICAgICAgfSxcbiAgICAgIFtgXiR7QkFTRV9VUkx9L2tpbm9kZS5jc3NgXToge1xuICAgICAgICB0YXJnZXQ6IFBST1hZX1VSTCxcbiAgICAgICAgY2hhbmdlT3JpZ2luOiB0cnVlLFxuICAgICAgICByZXdyaXRlOiAocGF0aCkgPT4ge1xuICAgICAgICAgIGNvbnNvbGUubG9nKCdQcm94eWluZyAgY3NyZXF1ZXN0OicsIHBhdGgpO1xuICAgICAgICAgIHJldHVybiAnL2tpbm9kZS5jc3MnO1xuICAgICAgICB9LFxuICAgICAgfSxcbiAgICAgIC8vIFRoaXMgcm91dGUgd2lsbCBtYXRjaCBhbGwgb3RoZXIgSFRUUCByZXF1ZXN0cyB0byB0aGUgYmFja2VuZFxuICAgICAgW2BeJHtCQVNFX1VSTH0vKD8hKEB2aXRlL2NsaWVudHxzcmMvLip8bm9kZV9tb2R1bGVzLy4qfEByZWFjdC1yZWZyZXNofCQpKWBdOiB7XG4gICAgICAgIHRhcmdldDogUFJPWFlfVVJMLFxuICAgICAgICBjaGFuZ2VPcmlnaW46IHRydWUsXG4gICAgICB9LFxuXG4gICAgfSxcblxuXG4gIH0sXG59KTtcbiJdLAogICJtYXBwaW5ncyI6ICI7QUFBaVYsU0FBUyxvQkFBb0I7QUFDOVcsU0FBUyxxQkFBcUI7QUFDOUIsT0FBTyxXQUFXO0FBY2xCLElBQU0sV0FBVztBQUdqQixJQUFNLGFBQWEsUUFBUSxJQUFJLGlCQUFpQix5QkFBeUIsUUFBUSxhQUFhLFdBQVc7QUFFekcsUUFBUSxJQUFJLDZCQUE2QixRQUFRLElBQUksZUFBZSxTQUFTO0FBRTdFLElBQU8sc0JBQVEsYUFBYTtBQUFBLEVBQzFCLFNBQVM7QUFBQSxJQUNQLGNBQWM7QUFBQSxNQUNaLFNBQVM7QUFBQSxRQUNQLFFBQVE7QUFBQSxNQUNWO0FBQUEsSUFDRixDQUFDO0FBQUEsSUFDRCxNQUFNO0FBQUEsRUFDUjtBQUFBLEVBQ0EsTUFBTTtBQUFBLEVBQ04sT0FBTztBQUFBLElBQ0wsZUFBZTtBQUFBLE1BQ2IsVUFBVSxDQUFDLFNBQVM7QUFBQSxJQUN0QjtBQUFBLEVBQ0Y7QUFBQSxFQUNBLFFBQVE7QUFBQSxJQUNOLE1BQU07QUFBQSxJQUNOLE9BQU87QUFBQSxNQUNMLENBQUMsSUFBSSxRQUFRLFNBQVMsR0FBRztBQUFBLFFBQ3ZCLFFBQVE7QUFBQSxRQUNSLGNBQWM7QUFBQSxRQUNkLFNBQVMsQ0FBQyxTQUFTO0FBQ2pCLGtCQUFRLElBQUksd0JBQXdCLElBQUk7QUFDeEMsaUJBQU87QUFBQSxRQUNUO0FBQUEsTUFDRjtBQUFBLE1BQ0EsQ0FBQyxJQUFJLFFBQVEsYUFBYSxHQUFHO0FBQUEsUUFDM0IsUUFBUTtBQUFBLFFBQ1IsY0FBYztBQUFBLFFBQ2QsU0FBUyxDQUFDLFNBQVM7QUFDakIsa0JBQVEsSUFBSSx3QkFBd0IsSUFBSTtBQUN4QyxpQkFBTztBQUFBLFFBQ1Q7QUFBQSxNQUNGO0FBQUE7QUFBQSxNQUVBLENBQUMsSUFBSSxRQUFRLDZEQUE2RCxHQUFHO0FBQUEsUUFDM0UsUUFBUTtBQUFBLFFBQ1IsY0FBYztBQUFBLE1BQ2hCO0FBQUEsSUFFRjtBQUFBLEVBR0Y7QUFDRixDQUFDOyIsCiAgIm5hbWVzIjogW10KfQo=
