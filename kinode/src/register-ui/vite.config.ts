import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import UnoCSS from '@unocss/vite'
import { presetUno, presetWind, presetIcons, transformerDirectives } from 'unocss'

export default defineConfig({
  plugins: [
    UnoCSS({
      presets: [presetUno(), presetWind(), presetIcons()],
      shortcuts: [
        {
          'flex-center': 'flex justify-center items-center',
          'flex-col-center': 'flex flex-col justify-center items-center',
        },
      ],
      rules: [
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
  ],
  // ...
  server: {
    proxy: {
      '/generate-networking-info': {
        target: 'http://localhost:8080/generate-networking-info',
        changeOrigin: true,
      },
      '/vet-keyfile': {
        target: 'http://localhost:8080/vet-keyfile',
        changeOrigin: true,
      },
      '/import-keyfile': {
        target: 'http://localhost:8080/import-keyfile',
        changeOrigin: true,
      },
      '/info': {
        target: 'http://localhost:8080/info',
        changeOrigin: true,
      },
      '/current-chain': {
        target: 'http://localhost:8080/current-chain',
        changeOrigin: true,
      },
      '/boot': {
        target: 'http://localhost:8080/boot',
        changeOrigin: true,
      }
    }
  }
})
