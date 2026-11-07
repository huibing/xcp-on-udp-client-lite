import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'

const r = (p) => fileURLToPath(new URL(p, import.meta.url))

// Browser harness: same UI sources, Tauri/plugin modules swapped for stand-ins
// so the real Experiment page can be driven without the desktop shell.
export default defineConfig({
  plugins: [vue()],
  server: { port: 5200, strictPort: true },
  resolve: {
    alias: {
      '@tauri-apps/api/core': r('./harness/mocks/tauri-core.js'),
      '@tauri-apps/api/event': r('./harness/mocks/tauri-event.js'),
      '@tauri-apps/plugin-dialog': r('./harness/mocks/dialog.js'),
      'vue-router': r('./harness/mocks/vue-router.js'),
    },
  },
})
