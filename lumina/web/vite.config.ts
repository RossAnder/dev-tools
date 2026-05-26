import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import vueDevTools from 'vite-plugin-vue-devtools'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig(({ command }) => ({
  // The SPA is served from the axum binary's root in production, so the
  // built asset URLs must be root-relative.
  base: '/',
  plugins: [
    vue({
      template: {
        compilerOptions: {
          comments: false,
        },
      },
      features: {
        optionsAPI: false,
      },
    }),
    // vue-devtools is dev-only — loading it during `vite build` adds ~1s of
    // plugin-init time for zero benefit (no devtools UI in a prod bundle).
    ...(command === 'serve' ? [vueDevTools()] : []),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  build: {
    target: 'esnext',
    reportCompressedSize: false,
    rolldownOptions: {
      output: {
        strictExecutionOrder: true,
      },
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
}))
