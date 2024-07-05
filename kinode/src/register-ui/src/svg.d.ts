import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import UnoCSS from '@unocss/vite'
import { presetUno, presetWind, presetIcons, transformerDirectives } from 'unocss'
import { NodeGlobalsPolyfillPlugin } from '@esbuild-plugins/node-globals-polyfill'

export default defineConfig({
  plugins: [
    NodeGlobalsPolyfillPlugin({
      buffer: true
    }),
    UnoCSS({
      presets: [presetUno(), presetWind(), presetIcons()],
      shortcuts: [
        {
          'flex-center': 'flex justify-center items-center',
          'flex-col-center': 'flex flex-col justify-center items-center',
        },
      ],
      theme: {
        // ... (keep your existing theme configuration)
      },
      transformers: [
        transformerDirectives()
      ],
    }),
    react(),
  ],
  build: {
    minify: 'terser',
    terserOptions: {
      compress: {
        drop_console: true,
        drop_debugger: true,
      },
    },
    rollupOptions: {
      output: {
        manualChunks: undefined,
        entryFileNames: 'assets/[name].js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: 'assets/[name].[ext]',
      },
    },
    cssCodeSplit: false,
  },
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, '')
      }
    }
  }
})