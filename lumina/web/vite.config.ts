import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import vueDevTools from 'vite-plugin-vue-devtools'

// https://vite.dev/config/
export default defineConfig({
  // The SPA is served from the axum binary's root in production, so the
  // built asset URLs must be root-relative.
  base: '/',
  plugins: [vue(), vueDevTools()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    // Dev-only cross-origin handling: proxy `/api/*` to the axum server on
    // 127.0.0.1:8080. This is the *entire* CORS story for development — the
    // server intentionally ships no tower-http cors layer (plan P15). The
    // axum routes are themselves mounted under `/api`, so the path is passed
    // through verbatim (no `rewrite`).
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true,
      },
    },
  },
})
