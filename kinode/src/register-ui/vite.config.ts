import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import UnoCSS from '@unocss/vite'
import { presetUno, presetWind, presetIcons, transformerDirectives } from 'unocss'
import { NodeGlobalsPolyfillPlugin } from '@esbuild-plugins/node-globals-polyfill'
// import viteCompression from 'vite-plugin-compression'

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
        colors: {
          'white': '#FFF5D9',
          'black': '#22211F',
          'orange': '#F35422',
          'transparent': 'transparent',
          'gray': '#7E7E7E',
        },
        font: {
          'sans': ['Barlow', 'ui-sans-serif', 'system-ui', '-apple-system', 'BlinkMacSystemFont', '"Segoe UI"', 'Roboto', '"Helvetica Neue"', 'Arial', '"Noto Sans"', 'sans-serif', '"Apple Color Emoji"', '"Segoe UI Emoji"', '"Segoe UI Symbol"', '"Noto Color Emoji"'],
          'serif': ['ui-serif', 'Georgia', 'Cambria', '"Times New Roman"', 'Times', 'serif'],
          'mono': ['ui-monospace', 'SFMono-Regular', 'Menlo', 'Monaco', 'Consolas', '"Liberation Mono"', '"Courier New"', 'monospace'],
          'heading': ['OpenSans', 'ui-sans-serif', 'system-ui', '-apple-system', 'BlinkMacSystemFont', '"Segoe UI"', 'Roboto', '"Helvetica Neue"', 'Arial', '"Noto Sans"', 'sans-serif', '"Apple Color Emoji"', '"Segoe UI Emoji"', '"Segoe UI Symbol"', '"Noto Color Emoji"'],
          'display': ['Futura', 'ui-sans-serif', 'system-ui', '-apple-system', 'BlinkMacSystemFont', '"Segoe UI"', 'Roboto', '"Helvetica Neue"', 'Arial', '"Noto Sans"', 'sans-serif', '"Apple Color Emoji"', '"Segoe UI Emoji"', '"Segoe UI Symbol"', '"Noto Color Emoji"'],
        },
      },
      transformers: [
        transformerDirectives()
      ],
    }),
    react(),
    // viteCompression({
    //   algorithm: 'gzip',
    //   verbose: false,
    //   threshold: 10240,
    //   ext: '.gz',
    // }),
  ],
  build: {
    minify: 'terser',
    // rollupOptions: {
    //   output: {
    //     manualChunks: {
    //       // vendor: ['react', 'react-dom', 'react-router-dom'],
    //     },
    //   },
    // },
    cssCodeSplit: true,
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
