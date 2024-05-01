import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import UnoCSS from 'unocss/vite'

export default defineConfig({
  plugins: [
    UnoCSS(), // must be BEFORE react()
    react()
  ],
  // ...
})
