import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue()],
  server: {
    // Caddy forwards the original Host header (e.g. myapp.test) to Vite.
    // Vite blocks unknown hosts by default, so allow our local dev TLD.
    // We run behind a local reverse proxy and intentionally serve arbitrary `*.test` hosts.
    // This is safe for a desktop-local dev tool, and avoids a brittle host allowlist.
    allowedHosts: true,
  },
})
