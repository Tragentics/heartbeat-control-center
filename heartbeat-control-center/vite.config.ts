import { defineConfig } from 'vite'

// Tauri expects a fixed dev port; fail fast instead of silently picking another.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
  },
  build: {
    target: 'es2022',
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: false,
    chunkSizeWarningLimit: 600,
  },
})
