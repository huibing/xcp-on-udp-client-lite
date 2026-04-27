import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [vue()],
  // Tauri expects a fixed port during development
  server: {
    port: 5173,
    strictPort: true,
  },
  // Produce relative paths for Tauri bundle
  base: './',
})
